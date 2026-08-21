import Foundation

/// The two languages the interface is written in.
///
/// Endonyms in the picker, never translated: a language menu that says
/// "Chinese" is a menu you have to already read English to use.
public enum UILocale: String, CaseIterable, Sendable {
    case zh
    case en

    public var endonym: String {
        switch self {
        case .zh: "中文"
        case .en: "English"
        }
    }

    /// The system's preferred language, in the sense the user means it: any
    /// Chinese tag is `zh`, everything else falls back to English.
    ///
    /// `Locale.preferredLanguages` is ordered by preference, so the first hit
    /// wins. This is the *system* setting rather than the process environment —
    /// a GUI app launched from Finder inherits almost no environment, and
    /// reading `LANG` there answers "English" on a Chinese Mac.
    public static var system: UILocale {
        for tag in Foundation.Locale.preferredLanguages {
            let lower = tag.lowercased()
            if lower.hasPrefix("zh") { return .zh }
            if lower.hasPrefix("en") { return .en }
        }
        return .en
    }
}

/// Every string the interface can show.
///
/// Ported from `lib/i18n/{en,zh}.ts`, but not the mechanism: there, the
/// guarantee that a locale is complete comes from `Record<MessageKey, string>`
/// and `tsc --noEmit`. Here it comes from an exhaustive `switch`, which is
/// strictly stronger — the placeholders are associated values, so a message that
/// takes a count cannot be used without one, and a rename cannot leave a
/// `{name}` in the template that nothing substitutes.
///
/// Deliberately absent, because this app does not render them:
///   * `titlebar.*` — the windows have real system frames, so macOS draws these.
///   * `settings.paste.*` — Wayland/X11 diagnostics for the Linux paste path.
///   * `setup.*` — the first-run wizard, which is not part of this rewrite.
///   * `anchor.*` / `bar.drag` — the bar is bottom-centred and not draggable.
/// Transcription languages and model names are not here either; they are data,
/// and they read the same in both locales.
public enum Msg: Sendable, Hashable {
    // MARK: Shared
    case appName
    case loading
    case settings
    case copy
    case clear
    case download
    case resume
    case yes
    case no
    case none
    case unknown

    // MARK: Model status
    case modelStatusAvailable
    case modelStatusDownloading
    case modelStatusInstalled
    case modelStatusPaused
    case modelStatusError

    // MARK: ASR runtimes
    case backendNano
    case backendSenseVoice

    // MARK: Download progress
    case downloadPreparing
    case downloadDownloaded(size: String)
    case downloadModel
    case downloadPaused
    case downloadPausing
    case downloadInstalled

    // MARK: Main window
    case newSession
    case noSession
    case sessionTitle(time: String)
    case sessionUntitled
    case showVoiceBar
    case hideVoiceBar

    case sidebarNoMatches
    case sidebarNoArchived
    case sidebarNoSessions

    case emptyTitle
    case emptyBody

    case statusReady
    case statusListening
    case statusTranscribing
    case statusSaved
    case statusArchived(title: String)
    case statusRestored(title: String)

    case transcriptFormat
    case transcriptFormatting
    case transcriptRedo
    case transcriptFormatTitle
    case transcriptFormatDisabled
    case transcriptFormatted(preset: String)
    case transcriptLivePartial

    case deckTalk
    case deckStop
    case deckModel
    case deckLanguage
    case deckMicrophone
    case deckMicrophoneDefault(name: String)
    case deckMicrophoneNone
    case deckNoMicrophone
    case deckInput
    case deckIdle

    // MARK: Search, filters, archive
    case searchPlaceholder
    case searchClear
    case searchFilters
    case searchReset
    case archive
    case restore
    case archiveTitle(title: String)
    case restoreTitle(title: String)
    case filterLanguage
    case anyLanguage
    case filterModel
    case anyModel
    case fromDate
    case toDate
    case dateSeparator
    case archiveScope
    case scopeActive
    case scopeArchived
    case scopeAll
    case archivedTag

    case matches(count: Int)
    case sessionsCount(count: Int)
    case searchSummary(matches: String, sessions: String)
    case searchTruncated

    case searchEmptyTitle(query: String)
    case searchEmptyFilters
    case searchEmptyArchived(scope: String)
    case searchEmptyHint

    // MARK: Settings
    case settingsTitle
    case settingsReload
    case settingsSaved

    case tabGeneral
    case tabModels
    case tabServices
    case tabAbout

    case modelsTitle

    case defaultsTitle
    case defaultsModel
    case defaultsLanguage
    case defaultsRuntime
    case defaultsUILocale

    case cloudTitle
    case cloudDesc
    case cloudPresetLocal
    case cloudBaseURL
    case cloudModel
    case cloudLanguageHint
    case cloudLanguageHintPlaceholder
    case cloudAPIKey
    case cloudAPIKeyPlaceholder

    case llmTitle
    case llmDesc
    case llmBaseURL
    case llmModel
    case llmPreset
    case llmAPIKey
    case llmAPIKeyStored
    case llmAPIKeyKeep
    case llmAPIKeyOptional
    case llmAPIKeyCleared
    case llmKeychain

    case storageTitle
    case storageRetainAudio
    case storageAutoPaste
    case storageLiveInsert
    case storageLiveInsertHint
    case storageSave

    // MARK: Push-to-talk
    case hotkeyTitle
    case hotkeyDesc
    case hotkeyChange
    case hotkeyRecording
    case hotkeyRecordingHint
    case hotkeyReset
    case hotkeyKeySpace
    case hotkeyNeedsModifier
    case hotkeyUnsupportedKey
    case hotkeyLive(chord: String)
    case hotkeyNotBound
    case hotkeyListener
    case hotkeyBackendEventTap
    case hotkeyBackendNone
    /// macOS-only, and the reason the whole feature can be inert: an event tap
    /// needs Accessibility, and without it push-to-talk fails silently.
    case hotkeyNeedsAccessibility
    case hotkeyOpenAccessibility

    // MARK: Microphone
    case micNeedsPermission
    case micOpenSettings

    // MARK: Trial and runtime
    case trialTitle
    case trialLicensed
    case trialUsed(minutes: Int)
    case trialLimit(minutes: Int)
    case trialNote

    case runtimeTitle
    case runtimeReady
    case runtimeMissing
    case runtimeEngine
    case runtimeCompute
    case runtimeComputeCPU
    case runtimePlatform

    // MARK: Voice bar
    case barHint
    case barNoTranscript

    // MARK: Menu bar
    case trayIdle
    case trayRecording
    case trayTranscribing
    case trayOpen
    case trayQuit

    // MARK: Honesty about the stub core
    /// Shown wherever a transcript comes from `StubTranscriptionEngine`. Without
    /// it, a screenshot of this build is indistinguishable from a screenshot of
    /// a working one, which is the failure this whole app is meant to avoid.
    case stubCoreNotice
}

extension Msg {
    public func text(in locale: UILocale) -> String {
        switch locale {
        case .en: english
        case .zh: chinese
        }
    }

    // swiftlint:disable:next cyclomatic_complexity function_body_length
    var english: String {
        switch self {
        case .appName: "Fun ASR"
        case .loading: "Loading Fun ASR"
        case .settings: "Settings"
        case .copy: "Copy"
        case .clear: "Clear"
        case .download: "Download"
        case .resume: "Resume"
        case .yes: "yes"
        case .no: "no"
        case .none: "none"
        case .unknown: "unknown"

        case .modelStatusAvailable: "Available"
        case .modelStatusDownloading: "Downloading"
        case .modelStatusInstalled: "Installed"
        case .modelStatusPaused: "Paused"
        case .modelStatusError: "Error"

        case .backendNano: "Fun-ASR-Nano (accurate)"
        case .backendSenseVoice: "SenseVoiceSmall (fast)"

        case .downloadPreparing: "Preparing download"
        case .downloadDownloaded(let size): "\(size) downloaded"
        case .downloadModel: "Downloading model from Hugging Face"
        case .downloadPaused: "Download paused"
        case .downloadPausing: "Pausing download…"
        case .downloadInstalled: "Model installed"

        case .newSession: "New Session"
        case .noSession: "No session"
        case .sessionTitle(let time): "Session \(time)"
        case .sessionUntitled: "Session"
        case .showVoiceBar: "Show the floating voice bar"
        case .hideVoiceBar: "Hide the voice bar"

        case .sidebarNoMatches: "No sessions match these filters."
        case .sidebarNoArchived: "Nothing archived yet."
        case .sidebarNoSessions: "No sessions yet."

        case .emptyTitle: "Start a local transcription session"
        case .emptyBody:
            "Saved transcripts appear here by date. Set the model, language and microphone below, then hold the push-to-talk key."

        case .statusReady: "Ready"
        case .statusListening: "Listening"
        case .statusTranscribing: "Transcribing"
        case .statusSaved: "Saved"
        case .statusArchived(let title):
            "Archived “\(title)”. It is still searchable and can be restored."
        case .statusRestored(let title): "Restored “\(title)” to the session list."

        case .transcriptFormat: "Format"
        case .transcriptFormatting: "Formatting"
        case .transcriptRedo: "Redo"
        case .transcriptFormatTitle: "Format as Markdown"
        case .transcriptFormatDisabled: "Configure an LLM in Settings first"
        case .transcriptFormatted(let preset): "Formatted · \(preset)"
        case .transcriptLivePartial: "Live partial"

        case .deckTalk: "Talk"
        case .deckStop: "Stop"
        case .deckModel: "Transcription model"
        case .deckLanguage: "Transcription language"
        case .deckMicrophone: "Microphone"
        case .deckMicrophoneDefault(let name): "Microphone · default is \(name)"
        case .deckMicrophoneNone: "Default microphone"
        case .deckNoMicrophone: "No microphone detected."
        case .deckInput: "Input"
        case .deckIdle: "idle"

        case .searchPlaceholder: "Search transcripts"
        case .searchClear: "Clear search"
        case .searchFilters: "Filters"
        case .searchReset: "Reset"
        case .archive: "Archive"
        case .restore: "Restore"
        case .archiveTitle(let title):
            "Hide “\(title)” from the main list. Nothing is deleted — the transcripts stay searchable and can be restored."
        case .restoreTitle(let title): "Move “\(title)” back into the main list"
        case .filterLanguage: "Filter by language"
        case .anyLanguage: "Any language"
        case .filterModel: "Filter by model"
        case .anyModel: "Any model"
        case .fromDate: "From date"
        case .toDate: "To date"
        case .dateSeparator: "to"
        case .archiveScope: "Show archived sessions"
        case .scopeActive: "Not archived"
        case .scopeArchived: "Archived"
        case .scopeAll: "Everything"
        case .archivedTag: "· archived"

        // Composed rather than pluralised, the same way the TypeScript
        // catalogue does it: `{n} matches` alone reads as "1 matches", and
        // Chinese has no plural form to select at all.
        case .matches(let count): count == 1 ? "1 match" : "\(count) matches"
        case .sessionsCount(let count): count == 1 ? "1 session" : "\(count) sessions"
        case .searchSummary(let matches, let sessions): "\(matches) in \(sessions)"
        case .searchTruncated: " · showing the most recent"

        case .searchEmptyTitle(let query): "No transcript contains “\(query)”"
        case .searchEmptyFilters:
            "Every transcript was searched, but the filters are hiding some sessions. Widen them, or clear them to search the whole history."
        case .searchEmptyArchived(let scope):
            "Archived sessions are not searched by default. Set the archive filter to \(scope) to include them."
        case .searchEmptyHint:
            "Search matches any run of characters inside a transcript, so a shorter fragment will usually find it."

        case .settingsTitle: "Settings"
        case .settingsReload: "Reload settings"
        case .settingsSaved: "Settings saved."

        case .tabGeneral: "General"
        case .tabModels: "Models"
        case .tabServices: "Services"
        case .tabAbout: "About"

        case .modelsTitle: "Models"

        case .defaultsTitle: "Defaults"
        case .defaultsModel: "Model"
        case .defaultsLanguage: "Language"
        case .defaultsRuntime: "Runtime"
        case .defaultsUILocale: "Interface language"

        case .cloudTitle: "Cloud transcription"
        case .cloudDesc:
            "Optional. Any OpenAI-compatible /v1/audio/transcriptions endpoint. Select “Cloud transcription” as the model to use it; the local model keeps running the live preview either way."
        case .cloudPresetLocal: "Local (Ollama)"
        case .cloudBaseURL: "Base URL"
        case .cloudModel: "Model"
        case .cloudLanguageHint: "Language hint"
        case .cloudLanguageHintPlaceholder: "blank = autodetect"
        case .cloudAPIKey: "API key"
        case .cloudAPIKeyPlaceholder: "Leave blank to keep the stored key"

        case .llmTitle: "Markdown formatting"
        case .llmDesc:
            "Optional. Any OpenAI-compatible endpoint works, including a local server. Transcripts are kept as spoken; the formatted version is stored alongside them."
        case .llmBaseURL: "Base URL"
        case .llmModel: "Model"
        case .llmPreset: "Preset"
        case .llmAPIKey: "API key"
        case .llmAPIKeyStored: "API key (stored)"
        case .llmAPIKeyKeep: "Leave blank to keep"
        case .llmAPIKeyOptional: "Optional for local servers"
        case .llmAPIKeyCleared: "API key cleared."
        case .llmKeychain: "Keys are stored in the system keychain, not in the app database."

        case .storageTitle: "Storage"
        case .storageRetainAudio: "Retain audio files"
        case .storageAutoPaste: "Voice bar auto-insert"
        case .storageLiveInsert: "Insert while speaking"
        case .storageLiveInsertHint:
            "Sends each phrase as you finish it, instead of the whole transcript at the end. Faster, but less accurate — short phrases decode worse than a full recording, and once text is in another app it cannot be corrected."
        case .storageSave: "Save Settings"

        case .hotkeyTitle: "Push-to-talk shortcut"
        case .hotkeyDesc: "Hold it to record anywhere; let go to transcribe."
        case .hotkeyChange: "Change"
        case .hotkeyRecording: "Press the new shortcut"
        case .hotkeyRecordingHint: "Modifier plus one key. Esc cancels."
        case .hotkeyReset: "Restore default"
        case .hotkeyKeySpace: "Space"
        case .hotkeyNeedsModifier:
            "Add Control, Option, Shift or Command. On its own that key would be taken away from every text field on the system."
        case .hotkeyUnsupportedKey:
            "That key cannot be used. Pick a letter, a digit, a function key or the space bar."
        case .hotkeyLive(let chord): "Live. Hold \(chord) to talk."
        case .hotkeyNotBound: "Not listening. Holding this will do nothing."
        case .hotkeyListener: "Listening via"
        case .hotkeyBackendEventTap: "macOS event tap"
        case .hotkeyBackendNone: "nothing"
        case .hotkeyNeedsAccessibility:
            "Fun ASR needs Accessibility to see the key while another app has the keyboard, and to type the transcript back into it."
        case .hotkeyOpenAccessibility: "Open Accessibility settings"

        case .micNeedsPermission: "Fun ASR needs the microphone to hear you."
        case .micOpenSettings: "Open Microphone settings"

        case .trialTitle: "Trial"
        case .trialLicensed: "Licensed. Thank you — that genuinely funds this."
        case .trialUsed(let minutes): "\(minutes) min"
        case .trialLimit(let minutes): "of \(minutes) min"
        case .trialNote:
            "Counts speech detected by VAD, not how long you held the key. Local transcription keeps working."

        case .runtimeTitle: "Runtime"
        case .runtimeReady: "Bundled runtime ready"
        case .runtimeMissing: "Runtime missing"
        case .runtimeEngine: "Engine"
        case .runtimeCompute: "Compute"
        case .runtimeComputeCPU: "CPU only"
        case .runtimePlatform: "Platform"

        case .barHint: "Hold to talk"
        case .barNoTranscript: "No transcript"

        case .trayIdle: "Idle"
        case .trayRecording: "Recording…"
        case .trayTranscribing: "Transcribing…"
        case .trayOpen: "Open Fun ASR"
        case .trayQuit: "Quit Fun ASR"

        case .stubCoreNotice:
            "Placeholder core: transcription is not wired up in this build. The microphone level is real; the words are not."
        }
    }

    // swiftlint:disable:next cyclomatic_complexity function_body_length
    var chinese: String {
        switch self {
        case .appName: "Fun ASR"
        case .loading: "正在启动 Fun ASR"
        case .settings: "设置"
        case .copy: "复制"
        case .clear: "清除"
        case .download: "下载"
        case .resume: "继续下载"
        case .yes: "是"
        case .no: "否"
        case .none: "无"
        case .unknown: "未知"

        case .modelStatusAvailable: "可下载"
        case .modelStatusDownloading: "下载中"
        case .modelStatusInstalled: "已安装"
        case .modelStatusPaused: "已暂停"
        case .modelStatusError: "出错"

        case .backendNano: "Fun-ASR-Nano（精准）"
        case .backendSenseVoice: "SenseVoiceSmall（快速）"

        case .downloadPreparing: "准备下载"
        case .downloadDownloaded(let size): "已下载 \(size)"
        case .downloadModel: "正在从 Hugging Face 下载模型"
        case .downloadPaused: "下载已暂停"
        case .downloadPausing: "正在暂停下载…"
        case .downloadInstalled: "模型已安装"

        case .newSession: "新建会话"
        case .noSession: "暂无会话"
        case .sessionTitle(let time): "会话 \(time)"
        case .sessionUntitled: "会话"
        case .showVoiceBar: "显示悬浮语音条"
        case .hideVoiceBar: "隐藏语音条"

        case .sidebarNoMatches: "没有会话符合当前筛选。"
        case .sidebarNoArchived: "还没有归档任何会话。"
        case .sidebarNoSessions: "还没有会话。"

        case .emptyTitle: "开始本地转写"
        case .emptyBody: "转写记录按日期存在这里。在下方选好模型、语言和麦克风，然后按住快捷键说话。"

        case .statusReady: "就绪"
        case .statusListening: "聆听中"
        case .statusTranscribing: "转写中"
        case .statusSaved: "已保存"
        case .statusArchived(let title): "已归档“\(title)”。内容没有删除，照样能搜到，也随时可以恢复。"
        case .statusRestored(let title): "已把“\(title)”移回会话列表。"

        case .transcriptFormat: "整理"
        case .transcriptFormatting: "整理中"
        case .transcriptRedo: "重新整理"
        case .transcriptFormatTitle: "整理成 Markdown"
        case .transcriptFormatDisabled: "请先在设置里配置 LLM"
        case .transcriptFormatted(let preset): "已整理 · \(preset)"
        case .transcriptLivePartial: "实时预览"

        case .deckTalk: "说话"
        case .deckStop: "停止"
        case .deckModel: "转写模型"
        case .deckLanguage: "转写语言"
        case .deckMicrophone: "麦克风"
        case .deckMicrophoneDefault(let name): "麦克风 · 默认为 \(name)"
        case .deckMicrophoneNone: "默认麦克风"
        case .deckNoMicrophone: "没有找到麦克风，请检查系统声音设置。"
        case .deckInput: "输入"
        case .deckIdle: "静默"

        case .searchPlaceholder: "搜索转写内容"
        case .searchClear: "清除搜索"
        case .searchFilters: "筛选"
        case .searchReset: "重置"
        case .archive: "归档"
        case .restore: "取消归档"
        case .archiveTitle(let title): "把“\(title)”从主列表里收起。不会删除任何内容，转写照样能搜到，也随时可以恢复。"
        case .restoreTitle(let title): "把“\(title)”移回主列表"
        case .filterLanguage: "按语言筛选"
        case .anyLanguage: "所有语言"
        case .filterModel: "按模型筛选"
        case .anyModel: "所有模型"
        case .fromDate: "起始日期"
        case .toDate: "结束日期"
        case .dateSeparator: "至"
        case .archiveScope: "归档范围"
        case .scopeActive: "未归档"
        case .scopeArchived: "已归档"
        case .scopeAll: "全部"
        case .archivedTag: "· 已归档"

        case .matches(let count): "\(count) 条结果"
        case .sessionsCount(let count): "\(count) 个会话"
        case .searchSummary(let matches, let sessions): "在 \(sessions)中找到 \(matches)"
        case .searchTruncated: " · 仅显示最近的"

        case .searchEmptyTitle(let query): "没有转写内容包含“\(query)”"
        case .searchEmptyFilters: "整个历史都搜过了，但筛选条件挡掉了一部分会话。放宽或清空筛选再试。"
        case .searchEmptyArchived(let scope): "已归档的会话默认不参与搜索。把归档范围改成“\(scope)”就会一起搜。"
        case .searchEmptyHint: "搜索会匹配转写里的任意连续片段，换个更短的词通常就能找到。"

        case .settingsTitle: "设置"
        case .settingsReload: "重新载入设置"
        case .settingsSaved: "设置已保存。"

        case .tabGeneral: "通用"
        case .tabModels: "模型"
        case .tabServices: "服务"
        case .tabAbout: "关于"

        case .modelsTitle: "模型"

        case .defaultsTitle: "默认设置"
        case .defaultsModel: "模型"
        case .defaultsLanguage: "语言"
        case .defaultsRuntime: "运行时"
        case .defaultsUILocale: "界面语言"

        case .cloudTitle: "云端转写"
        case .cloudDesc:
            "可选。支持任何 OpenAI 兼容的 /v1/audio/transcriptions 接口。把转写模型选成 Cloud transcription 就会走云端；实时预览始终由本地模型负责。"
        case .cloudPresetLocal: "本地 (Ollama)"
        case .cloudBaseURL: "Base URL"
        case .cloudModel: "模型"
        case .cloudLanguageHint: "语言提示"
        case .cloudLanguageHintPlaceholder: "留空则自动识别"
        case .cloudAPIKey: "API key"
        case .cloudAPIKeyPlaceholder: "留空则沿用已保存的 key"

        case .llmTitle: "Markdown 整理"
        case .llmDesc: "可选。任何 OpenAI 兼容接口都可以，本地服务也行。原始转写一字不改地保留，整理后的版本单独存放。"
        case .llmBaseURL: "Base URL"
        case .llmModel: "模型"
        case .llmPreset: "预设"
        case .llmAPIKey: "API key"
        case .llmAPIKeyStored: "API key（已保存）"
        case .llmAPIKeyKeep: "留空则不修改"
        case .llmAPIKeyOptional: "本地服务可留空"
        case .llmAPIKeyCleared: "API key 已清除。"
        case .llmKeychain: "API key 存在系统钥匙串里，不会写进应用数据库。"

        case .storageTitle: "存储"
        case .storageRetainAudio: "保留录音文件"
        case .storageAutoPaste: "语音条自动输入"
        case .storageLiveInsert: "边说边输入"
        case .storageLiveInsertHint:
            "说完一句就送出一句，不再等整段说完一次性送出。更快，但更不准——短句的识别效果不如整段录音，而且字一旦进了别的程序就改不回来。"
        case .storageSave: "保存设置"

        case .hotkeyTitle: "按住说话快捷键"
        case .hotkeyDesc: "在任何界面按住它录音，松开就转写。"
        case .hotkeyChange: "更改"
        case .hotkeyRecording: "按下新快捷键"
        case .hotkeyRecordingHint: "修饰键加一个普通键，Esc 取消。"
        case .hotkeyReset: "恢复默认"
        case .hotkeyKeySpace: "空格"
        case .hotkeyNeedsModifier: "得配一个 Control / Option / Shift / Command，不然这个键在所有输入框里都按不出来了。"
        case .hotkeyUnsupportedKey: "这个键不能用，换字母、数字、功能键或空格。"
        case .hotkeyLive(let chord): "已生效，按住 \(chord) 说话。"
        case .hotkeyNotBound: "没在监听，按住不会有反应。"
        case .hotkeyListener: "监听方式"
        case .hotkeyBackendEventTap: "macOS 事件监听"
        case .hotkeyBackendNone: "无"
        case .hotkeyNeedsAccessibility: "需要「辅助功能」权限：别的程序占着键盘时要能看到这个键，转写完还要把文字打回去。"
        case .hotkeyOpenAccessibility: "打开「辅助功能」设置"

        case .micNeedsPermission: "需要麦克风权限才能听到你说话。"
        case .micOpenSettings: "打开「麦克风」设置"

        case .trialTitle: "试用"
        case .trialLicensed: "已激活授权。谢谢支持，这确实养活了这个项目。"
        case .trialUsed(let minutes): "\(minutes) 分钟"
        case .trialLimit(let minutes): "共 \(minutes) 分钟"
        case .trialNote: "按 VAD 识别到的说话时长计算，不是按住快捷键的时长。额度用完后本地转写照常可用。"

        case .runtimeTitle: "运行时"
        case .runtimeReady: "内置运行时就绪"
        case .runtimeMissing: "运行时缺失"
        case .runtimeEngine: "引擎"
        case .runtimeCompute: "计算"
        case .runtimeComputeCPU: "仅 CPU"
        case .runtimePlatform: "平台"

        case .barHint: "按住说话"
        case .barNoTranscript: "没有转写结果"

        case .trayIdle: "空闲"
        case .trayRecording: "正在录音…"
        case .trayTranscribing: "正在转写…"
        case .trayOpen: "打开主窗口"
        case .trayQuit: "退出 Fun ASR"

        case .stubCoreNotice: "占位核心：这一版还没接上转写。麦克风电平是真的，文字不是。"
        }
    }
}

/// Resolves messages against the current locale.
///
/// A value rather than a global so a view can be rendered in a locale that is
/// not the app's — which is what the screenshot rig needs, and what a preview
/// wants.
public struct Localizer: Sendable {
    public var locale: UILocale

    public init(locale: UILocale) { self.locale = locale }

    /// `l(.deckTalk)` at the call site.
    public func callAsFunction(_ message: Msg) -> String {
        message.text(in: locale)
    }
}

// MARK: - Data that needs a label

extension ModelStatus {
    public var message: Msg {
        switch self {
        case .available: .modelStatusAvailable
        case .downloading: .modelStatusDownloading
        case .installed: .modelStatusInstalled
        case .paused: .modelStatusPaused
        case .error: .modelStatusError
        }
    }
}

extension ArchiveScope {
    public var message: Msg {
        switch self {
        case .active: .scopeActive
        case .archived: .scopeArchived
        case .all: .scopeAll
        }
    }
}

extension ASRBackend {
    /// The model name is a proper noun and stays as it is in every locale; only
    /// the parenthetical — what choosing it costs you — is translated. An
    /// unknown backend falls back to its raw identifier, which is what a
    /// diagnostic wants.
    public static func label(_ backend: String, _ l: Localizer) -> String {
        switch backend {
        case nano: l(.backendNano)
        case senseVoice: l(.backendSenseVoice)
        default: backend
        }
    }
}
