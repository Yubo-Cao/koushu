/**
 * The canonical message catalogue.
 *
 * English is the source of truth: `MessageKey` is derived from this object, so
 * every other locale is a `Record<MessageKey, string>` and `tsc --noEmit` fails
 * on a missing or misspelt key. Keys are flat and dotted rather than nested —
 * a nested tree needs recursive template-literal types to stay type-safe, and
 * buys nothing at this size.
 *
 * Placeholders are `{name}` and are substituted by `t()`.
 *
 * What is deliberately *not* in here:
 *   - Transcription languages (`lib/types.ts`). They are endonyms already
 *     ("中文", "日本語") and are stored in the database as spoken-language ids.
 *   - Model names, repo ids, hotkey triggers, `session_type` — backend data.
 *   - Error strings raised by Rust. Translating them would mean touching
 *     `src-tauri/`, which is out of scope; they surface as-is.
 */
export const en = {
  // ---- Shared ------------------------------------------------------------
  "common.appName": "Fun ASR",
  "common.loading": "Loading Fun ASR Desktop",
  "common.settings": "Settings",
  "common.pause": "Pause",
  "common.copy": "Copy",
  "common.clear": "Clear",
  "common.download": "Download",
  "common.resume": "Resume",
  "common.installed": "Installed",
  "common.yes": "yes",
  "common.no": "no",
  "common.none": "none",
  "common.unknown": "unknown",

  "titlebar.minimise": "Minimise",
  "titlebar.maximise": "Maximise",
  "titlebar.close": "Close",

  // ---- Model status ------------------------------------------------------
  "model.status.available": "Available",
  "model.status.downloading": "Downloading",
  "model.status.installed": "Installed",
  "model.status.paused": "Paused",
  "model.status.error": "Error",
  "model.status.ready": "ready",

  // ---- ASR runtimes ------------------------------------------------------
  "backend.nano": "Fun-ASR-Nano (accurate)",
  "backend.sensevoice": "SenseVoiceSmall (fast)",

  // ---- Download progress -------------------------------------------------
  "download.preparing": "Preparing download",
  "download.downloaded": "{size} downloaded",
  "download.model": "Downloading model from Hugging Face",
  "download.paused": "Download paused",
  "download.pausing": "Pausing download...",
  "download.installed": "Model installed",

  // ---- Main window -------------------------------------------------------
  "home.newSession": "New Session",
  "home.noSession": "No session",
  "home.sessionTitle": "Session {time}",
  "home.sessionUntitled": "Session",
  "home.showVoiceBar": "Show the floating voice bar",

  "home.sidebar.noMatches": "No sessions match these filters.",
  "home.sidebar.noArchived": "Nothing archived yet.",
  "home.sidebar.noSessions": "No sessions yet.",

  "home.empty.title": "Start a local transcription session",
  "home.empty.body":
    "Saved transcripts appear here by date. Set the model, language and microphone on the bar below, then press Talk.",

  "home.status.ready": "Ready",
  "home.status.listening": "Listening",
  "home.status.transcribing": "Transcribing",
  "home.status.saved": "Saved",
  "home.status.noVoice": "No voice detected · input {db}",

  "home.transcript.format": "Format",
  "home.transcript.formatting": "Formatting",
  "home.transcript.redo": "Redo",
  "home.transcript.formatTitle": "Format as Markdown",
  "home.transcript.formatDisabled": "Configure an LLM in Settings first",
  "home.transcript.formatted": "Formatted · {preset}",
  "home.transcript.livePartial": "Live partial",

  "home.deck.talk": "Talk",
  "home.deck.stop": "Stop",
  "home.deck.model": "Transcription model",
  "home.deck.language": "Transcription language",
  "home.deck.microphone": "Microphone",
  "home.deck.microphoneDefault": "Microphone · default is {name}",
  "home.deck.microphoneNone": "Default microphone",
  "home.deck.noMicrophone": "No microphone detected by the native audio backend.",
  "home.deck.input": "Input",
  "home.deck.idle": "idle",

  // ---- Session search, filters and archive -------------------------------
  "search.placeholder": "Search transcripts",
  "search.clear": "Clear search",
  "search.filters": "Filters",
  "search.reset": "Reset",
  "search.archive": "Archive",
  "search.restore": "Restore",
  "search.archiveTitle":
    "Hide “{title}” from the main list. Nothing is deleted — the transcripts stay searchable and can be restored.",
  "search.restoreTitle": "Move “{title}” back into the main list",
  "search.filterLanguage": "Filter by language",
  "search.anyLanguage": "Any language",
  "search.filterModel": "Filter by model",
  "search.anyModel": "Any model",
  "search.fromDate": "From date",
  "search.toDate": "To date",
  "search.dateSeparator": "to",
  "search.archiveScope": "Show archived sessions",
  "search.scopeActive": "Not archived",
  "search.scopeArchived": "Archived",
  "search.scopeAll": "Everything",
  "search.archivedTag": "· archived",

  // Counts are composed rather than pluralised: `{n} matches` in one string
  // would read as "1 matches", and Chinese has no plural form to select at all.
  "search.matches": "{count} matches",
  "search.matchesOne": "{count} match",
  "search.sessions": "{count} sessions",
  "search.sessionsOne": "{count} session",
  "search.summary": "{matches} in {sessions}",
  "search.truncated": " · showing the most recent",

  "search.empty.title": "No transcript contains “{query}”",
  "search.empty.filters":
    "Every transcript was searched, but the filters are hiding some sessions. Widen them, or clear them to search the whole history.",
  "search.empty.archived":
    "Archived sessions are not searched by default. Set the archive filter to {scope} to include them.",
  "search.empty.hint":
    "Search matches any run of characters inside a transcript, so a shorter fragment will usually find it.",

  // ---- Settings window ---------------------------------------------------
  "settings.title": "Settings",
  "settings.loading": "Loading settings",
  "settings.reload": "Reload settings",
  "settings.saved": "Settings saved.",

  // Category rail. Labels are one word each: they sit in a 176px rail beside
  // the panels they open, and a phrase there competes with the panel titles.
  "settings.nav": "Settings categories",
  "settings.tab.general": "General",
  "settings.tab.models": "Models",
  "settings.tab.services": "Services",
  "settings.tab.about": "About",

  "settings.models.title": "Models",

  "settings.defaults.title": "Defaults",
  "settings.defaults.model": "Model",
  "settings.defaults.language": "Language",
  "settings.defaults.runtime": "Runtime",
  "settings.defaults.uiLocale": "Interface language",

  "settings.cloud.title": "Cloud transcription",
  "settings.cloud.descBefore": "Optional. Any OpenAI-compatible ",
  "settings.cloud.descAfter":
    " endpoint. Select “Cloud transcription” as the model to use it; the local model keeps running the live preview either way.",
  "settings.cloud.presetLocal": "Local (Ollama)",
  "settings.cloud.baseUrl": "Base URL",
  "settings.cloud.model": "Model",
  "settings.cloud.languageHint": "Language hint",
  "settings.cloud.languageHintPlaceholder": "blank = autodetect",
  "settings.cloud.apiKey": "API key",
  "settings.cloud.apiKeyPlaceholder": "Leave blank to keep the stored key",

  "settings.llm.title": "Markdown formatting",
  "settings.llm.desc":
    "Optional. Any OpenAI-compatible endpoint works, including a local server. Transcripts are kept as spoken; the formatted version is stored alongside them.",
  "settings.llm.baseUrl": "Base URL",
  "settings.llm.model": "Model",
  "settings.llm.preset": "Preset",
  "settings.llm.apiKey": "API key",
  "settings.llm.apiKeyStored": "API key (stored)",
  "settings.llm.apiKeyKeep": "Leave blank to keep",
  "settings.llm.apiKeyOptional": "Optional for local servers",
  "settings.llm.apiKeyCleared": "API key cleared.",
  "settings.llm.keychain": "Keys are stored in the system keychain, not in the app database.",

  "settings.storage.title": "Storage",
  "settings.storage.retainAudio": "Retain audio files",
  "settings.storage.autoPaste": "Floating bar auto-paste",
  "settings.storage.liveInsert": "Insert while speaking",
  // States the trade rather than advertising the feature. Live insertion sends
  // each phrase the moment it is decoded, so it never gets the accuracy that
  // comes from decoding the whole recording at once — measured on this
  // project's test clip, an isolated tail segment decoded to a completely
  // different sentence. And text already in another application cannot be
  // taken back: correcting it would mean synthesising backspaces into a
  // document whose caret the user may have moved.
  "settings.storage.liveInsertHint":
    "Sends each phrase as you finish it, instead of the whole transcript at the end. Faster, but less accurate — short phrases decode worse than a full recording, and once text is in another app it cannot be corrected.",
  "settings.storage.save": "Save Settings",

  "settings.trial.title": "Trial",
  "settings.trial.licensed": "Licensed. Thank you — that genuinely funds this.",
  "settings.trial.used": "{minutes} min",
  "settings.trial.limit": "of {minutes} min",
  "settings.trial.note":
    "Counts speech detected by VAD, not how long you held the key. Local transcription keeps working.",

  "settings.runtime.title": "Runtime",
  "settings.runtime.ready": "Bundled runtime ready",
  "settings.runtime.missing": "Runtime missing",
  "settings.runtime.engine": "Engine",
  "settings.runtime.compute": "Compute",
  "settings.runtime.computeCpu": "CPU only",
  "settings.runtime.platform": "Platform",

  "settings.paste.title": "Linux paste",
  "settings.paste.session": "Session",
  "settings.paste.wayland": "Wayland",
  "settings.paste.x11": "X11",
  "settings.paste.tools": "Tools",

  "settings.setup.title": "Setup",
  "settings.setup.showAgain": "Show Setup Again",
  "settings.setup.willShow": "Setup will show on next main-window load.",

  // ---- First-run setup ---------------------------------------------------
  "setup.version": "Fun ASR Desktop {version}",
  "setup.headline": "Welcome to local voice transcription.",
  "setup.lede":
    "Talk into your microphone, get fast local transcripts, keep history on this machine, and use the floating bar to paste text into any app.",

  "setup.feature.cpu.title": "CPU-only ASR",
  "setup.feature.cpu.text":
    "Bundles the official Fun-ASR llama.cpp runtime. No GPU, no Python, no CUDA.",
  "setup.feature.models.title": "Hugging Face models",
  "setup.feature.models.text": "Downloads Fun-ASR-Nano Q4_K on first setup.",
  "setup.feature.local.title": "Local transcripts",
  "setup.feature.local.text": "Saves sessions by date in local SQLite.",
  "setup.feature.private.title": "Private by default",
  "setup.feature.private.text": "No cloud service is needed for transcription.",

  "setup.card.title": "Install Default Model",
  "setup.card.subtitle": "Fun-ASR-Nano GGUF Q4_K from Hugging Face",
  "setup.card.runtime": "Runtime",
  "setup.card.runtimeBundled": "Bundled CPU runtime",
  "setup.card.model": "Model",
  "setup.card.download": "Download",
  "setup.card.downloadSize": "about 897 MB",
  "setup.card.status": "Status",
  "setup.card.clipboard": "Clipboard",
  "setup.card.clipboardFull": "copy and paste available",
  "setup.card.clipboardCopyOnly": "copy only fallback",
  "setup.card.note":
    "The runtime is bundled with the app. The model is downloaded once and stored in app data.",

  "setup.downloading": "Downloading the Fun-ASR-Nano Q4_K GGUF model from Hugging Face.",
  "setup.downloadingShort": "Downloading Fun-ASR-Nano from Hugging Face",
  "setup.paused": "Download paused. Resume when you are ready.",
  "setup.continue": "Continue",
  "setup.resume": "Resume Download",
  "setup.downloadAndContinue": "Download Model and Continue",
  "setup.skip": "Continue Without Model",

  // ---- Voice bar ---------------------------------------------------------
  "bar.talk": "Talk",
  "bar.stop": "Stop",
  "bar.hide": "Hide",
  "bar.noHotkey": "no hotkey",
  "bar.inputLevel": "input level",
  "bar.drag": "Drag to move · {anchor}",
  "bar.noSpeech": "No speech",
  "bar.noTranscript": "No transcript",
  "bar.sessionTitle": "Voice {time}",

  "anchor.top-left": "top left",
  "anchor.top-center": "top centre",
  "anchor.top-right": "top right",
  "anchor.bottom-left": "bottom left",
  "anchor.bottom-center": "bottom centre",
  "anchor.bottom-right": "bottom right",
} as const;

/** Every key the UI may ask for. Derived, so the catalogue cannot drift. */
export type MessageKey = keyof typeof en;

/** The shape every locale has to fill in completely. */
export type Messages = Record<MessageKey, string>;
