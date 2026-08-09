"use client";

import { Cpu, Download, HardDrive, RefreshCw, RotateCcw, Save } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/Button";
import { TitleBar } from "@/components/TitleBar";
import { DownloadProgress } from "@/components/DownloadProgress";
import { DEFAULT_BACKEND, backendLabel } from "@/lib/backends";
import { formatBytes, modelStatusLabel } from "@/lib/format";
import { LOCALES, useI18n, type MessageKey } from "@/lib/i18n";
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
const ASR_PRESETS: {
  /** Vendor name, shown as-is. */
  label: string;
  /** Set only where the chip name is a description rather than a vendor. */
  labelKey?: MessageKey;
  baseUrl: string;
  model: string;
}[] = [
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
    labelKey: "settings.cloud.presetLocal",
    label: "Local (Ollama)",
    baseUrl: "http://localhost:11434/v1",
    model: "whisper",
  },
];

export default function SettingsPage() {
  const { t, locale, setLocale } = useI18n();
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
        message: t("download.model"),
      });
    } else if (event.event === "paused") {
      setDownload({
        modelId: event.data.modelId,
        active: false,
        paused: true,
        downloadedBytes: event.data.downloadedBytes,
        totalBytes: event.data.totalBytes,
        message: t("download.paused"),
      });
    } else if (event.event === "finished") {
      setDownload({
        modelId: event.data.modelId,
        active: false,
        paused: false,
        downloadedBytes: event.data.downloadedBytes,
        totalBytes: event.data.totalBytes,
        message: t("download.installed"),
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
    setMessage(t("download.model"));
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
      setMessage(updated.status === "installed" ? t("download.installed") : t("download.paused"));
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(null);
    }
  }

  async function pauseDownload(modelId: string) {
    setDownload((current) => (current ? { ...current, message: t("download.pausing") } : current));
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
      setMessage(t("settings.saved"));
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(null);
    }
  }

  async function showSetupAgain() {
    await resetOnboarding();
    setMessage(t("settings.setup.willShow"));
  }

  if (!bootstrap) {
    return (
      <div className="flex h-dvh flex-col">
        <TitleBar brand={<h1 className="t-title text-ctl font-semibold">{t("settings.title")}</h1>} />
        <main className="flex flex-1 items-center justify-center text-ctl text-smoke">
          {t("settings.loading")}
        </main>
      </div>
    );
  }

  return (
    <div className="flex h-dvh flex-col">
      {/* The window has no system frame, so this bar is the frame: it carries
          the title, it is the drag handle, and on Linux it draws the buttons.
          The old page heading — a 26px "Settings" over a sentence describing
          the panels below it — is gone; the panels are labelled, and a title
          bar already says which window this is. */}
      <TitleBar
        brand={<h1 className="t-title text-ctl font-semibold">{t("settings.title")}</h1>}
        actions={
          <Button variant="ghost" size="sm" className="w-[26px] px-0" title={t("settings.reload")} onClick={refresh}>
            <RefreshCw size={14} />
          </Button>
        }
      />
      <main className="scrollbar-thin min-h-0 flex-1 overflow-y-auto px-3 py-3 md:px-4 md:py-4">

      {/*
        Three columns at desktop width, not one.

        The old layout was `xl:grid-cols-[1fr_330px]` — and this window opens at
        1080px and cannot go below 960, so `xl` (1280px) never matched and the
        page always rendered as a single stacked column. Every field stretched
        to the full window, four settings filled a screen, and reaching the LLM
        config meant scrolling past everything. That is a phone layout being
        shown on a desktop.

        The breakpoints are measured against the viewport this window actually
        has, not against Tailwind's defaults. The window opens at 1080 and this
        display runs at 1.25, so the CSS viewport is ~994px — under `lg`
        (1024px), which is why an `lg:` rule would silently never fire and the
        page would keep rendering as one column at its default size. Hence the
        explicit values: two columns from 640, three from 940. At the 960px
        minimum the viewport is ~880 and it steps back down to two.

        Models spans the full width because it is a list. The rest splits into
        configuration first and status last, which is also the order of how
        often you touch them.
      */}
      <div className="grid grid-cols-1 gap-3 min-[640px]:grid-cols-2 min-[940px]:grid-cols-3">
        <section className="space-y-3 min-[640px]:col-span-2 min-[940px]:col-span-3">
          <Panel title={t("settings.models.title")}>
            <div className="divide-y divide-line-soft">
              {bootstrap.models.map((model) => (
                <div key={model.id} className="grid gap-3 py-3 first:pt-0 last:pb-0 md:grid-cols-[minmax(0,1fr)_auto] md:items-center">
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <HardDrive size={15} className="shrink-0 text-moss" />
                      <h2 className="truncate text-ctl font-semibold">{model.name}</h2>
                    </div>
                    <p className="mt-0.5 truncate text-ui text-smoke">{model.repo_id}</p>
                    <p className="mt-0.5 truncate font-mono text-meta text-faint">{model.local_path}</p>
                    {model.last_error ? <p className="mt-1.5 text-ui text-rust">{model.last_error}</p> : null}
                    {download?.modelId === model.id ? (
                      <div className="mt-2.5 max-w-xl">
                        <DownloadProgress
                          download={download}
                          onPause={download.active ? () => pauseDownload(model.id) : undefined}
                        />
                      </div>
                    ) : null}
                  </div>
                  <div className="flex shrink-0 items-center justify-end gap-3">
                    <span className="tnum text-meta text-smoke">{formatBytes(model.size_bytes)}</span>
                    <Button
                      icon={<Download size={15} />}
                      disabled={busy !== null || model.status === "installed"}
                      onClick={() => install(model)}
                    >
                      {model.status === "paused" ? t("common.resume") : modelStatusLabel(model.status, t)}
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          </Panel>

        </section>

        {/* Configuration, left to right in the order it is usually touched. */}
        <div className="space-y-3">
          <Panel title={t("settings.defaults.title")}>
            <div className="grid grid-cols-2 gap-2.5">
              <Field label={t("settings.defaults.model")}>
                <select
                  className="field w-full"
                  value={defaultModel}
                  onChange={(event) => setDefaultModel(event.target.value)}
                >
                  {bootstrap.models.map((model) => (
                    <option key={model.id} value={model.id}>
                      {model.name.replace(/\s*[(（].*[)）]$/, "")}
                    </option>
                  ))}
                </select>
              </Field>
              <Field label={t("settings.defaults.language")}>
                <select
                  className="field w-full"
                  value={defaultLanguage}
                  onChange={(event) => setDefaultLanguage(event.target.value)}
                >
                  {languages.map((language) => (
                    <option key={language} value={language}>
                      {language}
                    </option>
                  ))}
                </select>
              </Field>
            </div>
            {/* Applied on selection rather than on Save. A language control that
                needs a second click to take effect leaves the user reading the
                language they were trying to leave, and the provider persists the
                choice to the settings table itself. */}
            <div className="mt-2.5">
              <Field label={t("settings.defaults.uiLocale")}>
                <select
                  className="field w-full"
                  value={locale}
                  onChange={(event) => setLocale(event.target.value as typeof locale)}
                >
                  {LOCALES.map((item) => (
                    <option key={item.id} value={item.id}>
                      {item.label}
                    </option>
                  ))}
                </select>
              </Field>
            </div>
            <div className="mt-2.5 border-t border-line-soft pt-2.5">
              <Info
                label={t("settings.defaults.runtime")}
                value={backendLabel(
                  bootstrap.models.find((model) => model.id === defaultModel)?.backend || DEFAULT_BACKEND,
                  t,
                )}
              />
            </div>
          </Panel>

          <Panel title={t("settings.cloud.title")}>
            <p className="t-body mb-2.5 text-ui text-smoke">
              {t("settings.cloud.descBefore")}
              <code>/v1/audio/transcriptions</code>
              {t("settings.cloud.descAfter")}
            </p>
            {/* Four endpoints people actually use, as one row of chips. Typing a
                base URL from memory is the step where this feature gets
                abandoned. */}
            <div className="mb-2.5 flex flex-wrap gap-1.5">
              {ASR_PRESETS.map((preset) => (
                <Button
                  key={preset.label}
                  size="sm"
                  title={`${preset.baseUrl} · ${preset.model}`}
                  onClick={() => {
                    setAsrBaseUrl(preset.baseUrl);
                    setAsrModel(preset.model);
                  }}
                >
                  {preset.labelKey ? t(preset.labelKey) : preset.label}
                </Button>
              ))}
            </div>
            <div className="space-y-2">
              <Field label={t("settings.cloud.baseUrl")}>
                <input
                  className="field w-full"
                  placeholder="https://api.groq.com/openai/v1"
                  value={asrBaseUrl}
                  onChange={(event) => setAsrBaseUrl(event.target.value)}
                />
              </Field>
              {/* A model id and a two-letter language hint do not need the same
                  width as a URL. Pairing them halves the panel's height. */}
              <div className="grid grid-cols-2 gap-2">
                <Field label={t("settings.cloud.model")}>
                  <input
                    className="field w-full"
                    placeholder="whisper-large-v3-turbo"
                    value={asrModel}
                    onChange={(event) => setAsrModel(event.target.value)}
                  />
                </Field>
                <Field label={t("settings.cloud.languageHint")}>
                  <input
                    className="field w-full"
                    placeholder={t("settings.cloud.languageHintPlaceholder")}
                    value={asrLanguage}
                    onChange={(event) => setAsrLanguage(event.target.value)}
                  />
                </Field>
              </div>
              <Field label={t("settings.cloud.apiKey")}>
                <input
                  type="password"
                  className="field w-full"
                  placeholder={t("settings.cloud.apiKeyPlaceholder")}
                  value={asrKeyDraft}
                  onChange={(event) => setAsrKeyDraft(event.target.value)}
                />
              </Field>
            </div>
          </Panel>
        </div>

        <div className="space-y-3">
          <Panel title={t("settings.llm.title")}>
            <p className="t-body mb-2.5 text-ui text-smoke">
              {t("settings.llm.desc")}
            </p>
            <div className="space-y-2">
              <Field label={t("settings.llm.baseUrl")}>
                <input
                  className="field w-full"
                  placeholder="http://localhost:11434/v1"
                  value={llmBaseUrl}
                  onChange={(event) => setLlmBaseUrl(event.target.value)}
                />
              </Field>
              <div className="grid grid-cols-2 gap-2">
                <Field label={t("settings.llm.model")}>
                  <input
                    className="field w-full"
                    placeholder="qwen2.5:7b"
                    value={llmModel}
                    onChange={(event) => setLlmModel(event.target.value)}
                  />
                </Field>
                <Field label={t("settings.llm.preset")}>
                  <select
                    className="field w-full"
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
              </div>
              <p className="t-body text-meta text-smoke">
                {llm?.presets.find((preset) => preset.id === llmPreset)?.description}
              </p>
              <Field label={llm?.hasApiKey ? t("settings.llm.apiKeyStored") : t("settings.llm.apiKey")}>
                <div className="flex gap-2">
                  <input
                    type="password"
                    className="field min-w-0 flex-1"
                    placeholder={
                      llm?.hasApiKey
                        ? t("settings.llm.apiKeyKeep")
                        : t("settings.llm.apiKeyOptional")
                    }
                    value={apiKeyDraft}
                    onChange={(event) => setApiKeyDraft(event.target.value)}
                  />
                  {llm?.hasApiKey ? (
                    <Button
                      className="shrink-0"
                      onClick={async () => {
                        await setLlmApiKey(null);
                        setLlm(await getLlmSettings());
                        setMessage(t("settings.llm.apiKeyCleared"));
                      }}
                    >
                      {t("common.clear")}
                    </Button>
                  ) : null}
                </div>
              </Field>
              <p className="t-body text-meta text-smoke">
                {t("settings.llm.keychain")}
              </p>
            </div>
          </Panel>

          <Panel title={t("settings.storage.title")}>
            <Toggle label={t("settings.storage.retainAudio")} checked={retainAudio} onChange={setRetainAudio} />
            <Toggle label={t("settings.storage.autoPaste")} checked={autoPaste} onChange={setAutoPaste} />
            <Button className="mt-2.5 w-full" variant="primary" icon={<Save size={15} />} disabled={busy !== null} onClick={saveSettings}>
              {t("settings.storage.save")}
            </Button>
          </Panel>
        </div>

        {/* Status and diagnostics: read far more often than they are changed,
            so they get the narrow column and the tightest rows. On a two-column
            window this pair sits side by side under the configuration instead
            of stacking into a long tail. */}
        <div className="grid grid-cols-1 gap-3 self-start min-[640px]:col-span-2 min-[640px]:grid-cols-2 min-[940px]:col-span-1 min-[940px]:grid-cols-1">
          <Panel title={t("settings.trial.title")}>
            {trial?.licensed ? (
              <p className="t-body text-ui text-smoke">
                {t("settings.trial.licensed")}
              </p>
            ) : trial ? (
              <>
                <div className="mb-1.5 flex items-baseline justify-between">
                  <span className="tnum t-title text-head font-semibold">
                    {t("settings.trial.used", { minutes: Math.floor(trial.usedSeconds / 60) })}
                  </span>
                  <span className="tnum text-meta text-smoke">
                    {t("settings.trial.limit", { minutes: Math.round(trial.limitSeconds / 60) })}
                  </span>
                </div>
                <div className="h-1.5 overflow-hidden rounded-pill bg-track">
                  <div
                    className="h-full rounded-pill bg-accent transition-all"
                    style={{
                      width: `${Math.min(100, (trial.usedSeconds / trial.limitSeconds) * 100)}%`,
                    }}
                  />
                </div>
                <p className="t-body mt-2 text-meta text-smoke">
                  {t("settings.trial.note")}
                </p>
              </>
            ) : null}
          </Panel>

          <Panel title={t("settings.runtime.title")}>
            <div className="mb-2 flex items-center gap-2 text-ctl">
              <Cpu size={15} className={bootstrap.platform.bundled_asr ? "text-moss" : "text-rust"} />
              <span className="font-medium">
                {bootstrap.platform.bundled_asr
                  ? t("settings.runtime.ready")
                  : t("settings.runtime.missing")}
              </span>
            </div>
            <div className="space-y-1">
              <Info label={t("settings.runtime.engine")} value="llama.cpp (Fun-ASR)" />
              <Info label={t("settings.runtime.compute")} value={t("settings.runtime.computeCpu")} />
              <Info label={t("settings.runtime.platform")} value={`${bootstrap.platform.os} ${bootstrap.platform.arch}`} />
            </div>
          </Panel>

          <Panel title={t("settings.paste.title")}>
            <div className="space-y-1">
              <Info label={t("settings.paste.session")} value={bootstrap.platform.session_type || t("common.unknown")} />
              <Info label={t("settings.paste.wayland")} value={bootstrap.platform.wayland_display ? t("common.yes") : t("common.no")} />
              <Info label={t("settings.paste.x11")} value={bootstrap.platform.x11_display ? t("common.yes") : t("common.no")} />
              <Info label={t("settings.paste.tools")} value={bootstrap.platform.paste_tools.join(", ") || t("common.none")} />
            </div>
          </Panel>

          <Panel title={t("settings.setup.title")}>
            <Button className="w-full" icon={<RotateCcw size={15} />} onClick={showSetupAgain}>
              {t("settings.setup.showAgain")}
            </Button>
            {message ? (
              <p className="t-body mt-2.5 rounded-md bg-fill p-2.5 text-ui text-smoke">{message}</p>
            ) : null}
          </Panel>
        </div>
      </div>
      </main>
    </div>
  );
}

function Panel({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="glass rim rounded-lg p-3.5">
      <h2 className="t-head mb-3 text-head font-semibold">{title}</h2>
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
    <label className="mb-2.5 flex items-center justify-between gap-4 text-ctl">
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
      <span className="t-micro mb-1 block text-meta font-medium text-smoke">{label}</span>
      {children}
    </label>
  );
}

function Info({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline justify-between gap-4 text-ctl">
      <span className="shrink-0 text-smoke">{label}</span>
      <span className="min-w-0 truncate text-right font-medium text-ink" title={value}>
        {value}
      </span>
    </div>
  );
}
