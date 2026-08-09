"use client";

import {
  Boxes,
  Cloud,
  Cpu,
  Download,
  HardDrive,
  Info,
  RefreshCw,
  RotateCcw,
  Save,
  SlidersHorizontal,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/Button";
import { HotkeyRecorder } from "@/components/HotkeyRecorder";
import { TitleBar } from "@/components/TitleBar";
import { WindowFrame } from "@/components/WindowFrame";
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

/**
 * The categories, in the order they are usually needed.
 *
 * Four, because that is how many groups the existing panels genuinely fall
 * into — the count is not a target. Everything that decides what the next
 * recording does is General; the things you install are Models; the two
 * OpenAI-compatible endpoints are both Services and neither is used by most
 * people; the rest is status you read rather than settings you change.
 */
const TABS = [
  { id: "general", labelKey: "settings.tab.general", Icon: SlidersHorizontal },
  { id: "models", labelKey: "settings.tab.models", Icon: Boxes },
  { id: "services", labelKey: "settings.tab.services", Icon: Cloud },
  { id: "about", labelKey: "settings.tab.about", Icon: Info },
] as const satisfies readonly { id: string; labelKey: MessageKey; Icon: typeof Info }[];

type TabId = (typeof TABS)[number]["id"];

export default function SettingsPage() {
  const { t, locale, setLocale } = useI18n();
  const [bootstrap, setBootstrap] = useState<Bootstrap | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [message, setMessage] = useState("");
  // Component state, not a route: this is a static export and the window is
  // opened at a fixed `/settings` URL, so there is no segment to put it in.
  const [tab, setTab] = useState<TabId>("general");
  const [defaultModel, setDefaultModel] = useState("fun-asr-nano-2512");
  const [defaultLanguage, setDefaultLanguage] = useState("中文");
  const [retainAudio, setRetainAudio] = useState(false);
  const [autoPaste, setAutoPaste] = useState(true);
  // Off by default: it trades accuracy for latency, and a default that silently
  // lowers transcription quality is the wrong one to pick on the user's behalf.
  const [liveInsert, setLiveInsert] = useState(false);
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
    getTrialStatus()
      .then(setTrial)
      .catch(() => setTrial(null));
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
        setLiveInsert(data.settings["floating.liveInsert"] === "true");
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
      await setSetting("floating.liveInsert", liveInsert ? "true" : "false");
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
      <WindowFrame>
        <TitleBar
          brand={<h1 className="t-title text-ctl font-semibold">{t("settings.title")}</h1>}
        />
        <main className="flex flex-1 items-center justify-center text-ctl text-smoke">
          {t("settings.loading")}
        </main>
      </WindowFrame>
    );
  }

  const activeTab = TABS.find((item) => item.id === tab) ?? TABS[0];

  return (
    <WindowFrame>
      {/* The window has no system frame, so this bar is the frame: it carries
          the title, it is the drag handle, and on Linux it draws the buttons.
          Its left zone is exactly the rail's width, so the app identity sits
          over the categories and the section name sits over its panels. */}
      <TitleBar
        leadClassName="settings-lead pr-2"
        brand={<h1 className="t-title truncate text-ctl font-semibold">{t("settings.title")}</h1>}
        actions={
          <Button
            variant="ghost"
            size="sm"
            className="w-[26px] px-0"
            title={t("settings.reload")}
            onClick={refresh}
          >
            <RefreshCw size={14} />
          </Button>
        }
      >
        <span data-tauri-drag-region className="truncate text-ctl font-semibold">
          {t(activeTab.labelKey)}
        </span>
      </TitleBar>

      <div className="flex min-h-0 flex-1">
        <SettingsRail current={tab} onSelect={setTab} />

        <main
          id={`settings-panel-${tab}`}
          role="tabpanel"
          aria-labelledby={`settings-tab-${tab}`}
          // Focusable so the panel is reachable by Tab straight from the rail,
          // which is what the ARIA tabs pattern asks for whenever the panel can
          // scroll — and this one can.
          tabIndex={0}
          className="scrollbar-thin min-h-0 flex-1 overflow-y-auto px-3 py-3 md:px-4 md:py-4"
        >
          {/* Each tab states the width its own content wants, because they do
              not want the same one. Two short panels stretched across the whole
              window read as a page that failed to load; a pair of tall forms
              stacked in a 620px column reads as a page that needs scrolling for
              no reason.

              The second column appears at 1080px, and that number is checked
              against the window this actually runs in rather than picked from a
              framework's defaults. This window opens at 1116 CSS px, so a
              breakpoint anywhere above that would never fire at the default
              size and the second column would be dead code that looks like a
              bug. At 1080 the rail (176), the gutter (36) and the padding (32)
              still leave ~420 per column, which holds a Base URL without
              truncating it; below that it falls back to one. */}
          {tab === "general" ? (
            <TabBody className="max-w-[620px] space-y-3">
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
                {/* Applied on selection rather than on Save. A language control
                      that needs a second click to take effect leaves the user
                      reading the language they were trying to leave, and the
                      provider persists the choice to the settings table itself. */}
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
                  <Info2
                    label={t("settings.defaults.runtime")}
                    value={backendLabel(
                      bootstrap.models.find((model) => model.id === defaultModel)?.backend ||
                        DEFAULT_BACKEND,
                      t,
                    )}
                  />
                </div>
              </Panel>

              {/* Applied the moment a chord is recorded, not on Save. Whether
                  a global shortcut can actually be taken is something only the
                  desktop can answer, and the answer is worth nothing three
                  tabs and one button click after the key was pressed. */}
              <Panel title={t("settings.hotkey.title")}>
                <HotkeyRecorder
                  mac={bootstrap.platform.os === "macos"}
                  stored={String(bootstrap.settings["hotkey.pushToTalk"] || "")}
                />
              </Panel>

              <Panel title={t("settings.storage.title")}>
                <Toggle
                  label={t("settings.storage.retainAudio")}
                  checked={retainAudio}
                  onChange={setRetainAudio}
                />
                <Toggle
                  label={t("settings.storage.autoPaste")}
                  checked={autoPaste}
                  onChange={setAutoPaste}
                />
                <Toggle
                  label={t("settings.storage.liveInsert")}
                  hint={t("settings.storage.liveInsertHint")}
                  checked={liveInsert}
                  onChange={setLiveInsert}
                />
              </Panel>
            </TabBody>
          ) : null}

          {tab === "models" ? (
            <TabBody className="max-w-[980px] space-y-3">
              {/* A list wants the width; the two columns other tabs use would
                    only halve it. */}
              <div>
                <Panel title={t("settings.models.title")}>
                  <div className="divide-y divide-line-soft">
                    {bootstrap.models.map((model) => (
                      <div
                        key={model.id}
                        className="grid gap-3 py-3 first:pt-0 last:pb-0 min-[720px]:grid-cols-[minmax(0,1fr)_auto] min-[720px]:items-center"
                      >
                        <div className="min-w-0">
                          <div className="flex items-center gap-2">
                            <HardDrive size={15} className="shrink-0 text-moss" />
                            <h2 className="truncate text-ctl font-semibold">{model.name}</h2>
                          </div>
                          <p className="mt-0.5 truncate text-ui text-smoke">{model.repo_id}</p>
                          <p className="mt-0.5 truncate font-mono text-meta text-faint">
                            {model.local_path}
                          </p>
                          {model.last_error ? (
                            <p className="mt-1.5 text-ui text-rust">{model.last_error}</p>
                          ) : null}
                          {download?.modelId === model.id ? (
                            <div className="mt-2.5 max-w-xl">
                              <DownloadProgress
                                download={download}
                                onPause={
                                  download.active ? () => pauseDownload(model.id) : undefined
                                }
                              />
                            </div>
                          ) : null}
                        </div>
                        <div className="flex shrink-0 items-center justify-end gap-3">
                          <span className="tnum text-meta text-smoke">
                            {formatBytes(model.size_bytes)}
                          </span>
                          <Button
                            icon={<Download size={15} />}
                            disabled={busy !== null || model.status === "installed"}
                            onClick={() => install(model)}
                          >
                            {model.status === "paused"
                              ? t("common.resume")
                              : modelStatusLabel(model.status, t)}
                          </Button>
                        </div>
                      </div>
                    ))}
                  </div>
                </Panel>
              </div>

              <Panel title={t("settings.runtime.title")}>
                <div className="mb-2 flex items-center gap-2 text-ctl">
                  <Cpu
                    size={15}
                    className={bootstrap.platform.bundled_asr ? "text-moss" : "text-rust"}
                  />
                  <span className="font-medium">
                    {bootstrap.platform.bundled_asr
                      ? t("settings.runtime.ready")
                      : t("settings.runtime.missing")}
                  </span>
                </div>
                <div className="space-y-1">
                  <Info2 label={t("settings.runtime.engine")} value="llama.cpp (Fun-ASR)" />
                  <Info2
                    label={t("settings.runtime.compute")}
                    value={t("settings.runtime.computeCpu")}
                  />
                  <Info2
                    label={t("settings.runtime.platform")}
                    value={`${bootstrap.platform.os} ${bootstrap.platform.arch}`}
                  />
                </div>
              </Panel>
            </TabBody>
          ) : null}

          {tab === "services" ? (
            <TabBody className="grid max-w-[1000px] gap-3 min-[1080px]:grid-cols-2 min-[1080px]:items-start">
              <Panel title={t("settings.cloud.title")}>
                <p className="t-body mb-2.5 text-ui text-smoke">
                  {t("settings.cloud.descBefore")}
                  <code>/v1/audio/transcriptions</code>
                  {t("settings.cloud.descAfter")}
                </p>
                {/* Four endpoints people actually use, as one row of chips.
                      Typing a base URL from memory is the step where this
                      feature gets abandoned. */}
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
                  {/* A model id and a two-letter language hint do not need the
                        same width as a URL. Pairing them halves the panel. */}
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

              <Panel title={t("settings.llm.title")}>
                <p className="t-body mb-2.5 text-ui text-smoke">{t("settings.llm.desc")}</p>
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
                  <Field
                    label={
                      llm?.hasApiKey ? t("settings.llm.apiKeyStored") : t("settings.llm.apiKey")
                    }
                  >
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
                  <p className="t-body text-meta text-smoke">{t("settings.llm.keychain")}</p>
                </div>
              </Panel>
            </TabBody>
          ) : null}

          {tab === "about" ? (
            <TabBody className="grid max-w-[1000px] gap-3 min-[1080px]:grid-cols-2 min-[1080px]:items-start">
              <Panel title={t("settings.trial.title")}>
                {trial?.licensed ? (
                  <p className="t-body text-ui text-smoke">{t("settings.trial.licensed")}</p>
                ) : trial ? (
                  <>
                    <div className="mb-1.5 flex items-baseline justify-between">
                      <span className="tnum t-title text-head font-semibold">
                        {t("settings.trial.used", {
                          minutes: Math.floor(trial.usedSeconds / 60),
                        })}
                      </span>
                      <span className="tnum text-meta text-smoke">
                        {t("settings.trial.limit", {
                          minutes: Math.round(trial.limitSeconds / 60),
                        })}
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
                    <p className="t-body mt-2 text-meta text-smoke">{t("settings.trial.note")}</p>
                  </>
                ) : null}
              </Panel>

              <Panel title={t("settings.paste.title")}>
                <div className="space-y-1">
                  <Info2
                    label={t("settings.paste.session")}
                    value={bootstrap.platform.session_type || t("common.unknown")}
                  />
                  <Info2
                    label={t("settings.paste.wayland")}
                    value={bootstrap.platform.wayland_display ? t("common.yes") : t("common.no")}
                  />
                  <Info2
                    label={t("settings.paste.x11")}
                    value={bootstrap.platform.x11_display ? t("common.yes") : t("common.no")}
                  />
                  <Info2
                    label={t("settings.paste.tools")}
                    value={bootstrap.platform.paste_tools.join(", ") || t("common.none")}
                  />
                </div>
              </Panel>

              <Panel title={t("settings.setup.title")}>
                <Button className="w-full" icon={<RotateCcw size={15} />} onClick={showSetupAgain}>
                  {t("settings.setup.showAgain")}
                </Button>
              </Panel>
            </TabBody>
          ) : null}
        </main>
      </div>

      {/* Save lives here rather than inside one panel. Once the settings are
          split across four tabs, a button parked in Storage would silently be
          the thing that commits the Cloud form three tabs away. */}
      <footer className="footer-bar flex shrink-0 items-center gap-3 px-4 py-2.5">
        <p className="t-body min-w-0 flex-1 truncate text-ui text-smoke" title={message}>
          {message}
        </p>
        <Button
          className="shrink-0"
          variant="primary"
          icon={<Save size={15} />}
          disabled={busy !== null}
          onClick={saveSettings}
        >
          {t("settings.storage.save")}
        </Button>
      </footer>
    </WindowFrame>
  );
}

/**
 * The category rail.
 *
 * Selection follows focus, which is the ARIA tabs default and the right call
 * when switching panels costs nothing. Both axes of arrow key are handled: the
 * rail is vertical at every width this window can reach, but Left/Right is what
 * a lot of people reach for on a tab strip and refusing it buys nothing.
 */
function SettingsRail({ current, onSelect }: { current: TabId; onSelect: (tab: TabId) => void }) {
  const { t } = useI18n();
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const index = TABS.findIndex((tab) => tab.id === current);

  function move(next: number) {
    const wrapped = (next + TABS.length) % TABS.length;
    onSelect(TABS[wrapped].id);
    refs.current[wrapped]?.focus();
  }

  function onKeyDown(event: React.KeyboardEvent) {
    const keys: Record<string, number | undefined> = {
      ArrowDown: index + 1,
      ArrowRight: index + 1,
      ArrowUp: index - 1,
      ArrowLeft: index - 1,
      Home: 0,
      End: TABS.length - 1,
    };
    const next = keys[event.key];
    if (next === undefined) return;
    event.preventDefault();
    move(next);
  }

  return (
    <nav
      role="tablist"
      aria-orientation="vertical"
      aria-label={t("settings.nav")}
      onKeyDown={onKeyDown}
      className="settings-rail"
    >
      {TABS.map((tab, i) => {
        const selected = tab.id === current;
        const label = t(tab.labelKey);
        return (
          <button
            key={tab.id}
            ref={(node) => {
              refs.current[i] = node;
            }}
            type="button"
            role="tab"
            id={`settings-tab-${tab.id}`}
            aria-selected={selected}
            aria-controls={`settings-panel-${tab.id}`}
            // Roving tabindex: one stop for the whole rail, arrows move inside.
            tabIndex={selected ? 0 : -1}
            title={label}
            onClick={() => onSelect(tab.id)}
            className={[
              "press flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-ctl",
              // Same selected treatment as the session list in the main window:
              // a fill and a weight change, not a second pane of glass.
              selected ? "bg-fill-strong font-medium text-ink" : "text-ink-2 hover:bg-fill",
            ].join(" ")}
          >
            <tab.Icon size={15} className="shrink-0" />
            <span className="settings-tab-label min-w-0 truncate">{label}</span>
          </button>
        );
      })}
    </nav>
  );
}

/** Centres a tab's content and caps it at the measure that tab asked for. */
function TabBody({ className, children }: { className: string; children: React.ReactNode }) {
  return <div className={`mx-auto w-full ${className}`}>{children}</div>;
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
  hint,
  checked,
  onChange,
}: {
  label: string;
  /** Shown under the label. For settings whose cost is not obvious from a name. */
  hint?: string;
  checked: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <label className="mb-2.5 flex items-center justify-between gap-4 text-ctl last:mb-0">
      <span className="min-w-0">
        {label}
        {hint ? <span className="t-micro mt-0.5 block text-meta text-smoke">{hint}</span> : null}
      </span>
      <input
        className="switch press"
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

/** Named `Info2` because `Info` is the lucide icon used by the About tab. */
function Info2({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline justify-between gap-4 text-ctl">
      <span className="shrink-0 text-smoke">{label}</span>
      <span className="min-w-0 truncate text-right font-medium text-ink" title={value}>
        {value}
      </span>
    </div>
  );
}
