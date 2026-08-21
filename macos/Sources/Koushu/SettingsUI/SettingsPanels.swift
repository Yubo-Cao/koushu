import KoushuCore
import SwiftUI

// MARK: - General

struct GeneralSettings: View {
    @Bindable var app: AppModel
    @Bindable var draft: SettingsDraft

    var body: some View {
        Form {
            Section(app.l(.defaultsTitle)) {
                Picker(app.l(.defaultsModel), selection: $app.defaultModelID) {
                    ForEach(app.models) { Text($0.name).tag($0.id) }
                }
                Picker(app.l(.defaultsLanguage), selection: $app.defaultLanguage) {
                    ForEach(transcriptionLanguages, id: \.self) { Text($0).tag($0) }
                }
                // Applied on selection rather than on Save. A language control
                // that needs a second click to take effect leaves the user
                // reading the language they were trying to leave.
                Picker(app.l(.defaultsUILocale), selection: localeBinding) {
                    ForEach(UILocale.allCases, id: \.self) { Text($0.endonym).tag($0) }
                }
                LabeledContent(app.l(.defaultsRuntime), value: ASRBackend.label(app.runtime, app.l))
            }

            // Applied the moment a chord is recorded, not on Save. Whether a
            // global shortcut can actually be taken is something only the system
            // can answer, and the answer is worth nothing three tabs and one
            // button click after the key was pressed.
            Section(app.l(.hotkeyTitle)) {
                HotkeyRecorder(
                    app: app,
                    suspend: { NotificationCenter.default.post(name: .koushuSuspendHotkey, object: nil) },
                    apply: { chord in
                        NotificationCenter.default.post(
                            name: .koushuApplyHotkey,
                            object: nil,
                            userInfo: ["chord": chord.stored]
                        )
                    }
                )
                Text(app.l(.hotkeyDesc))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                LabeledContent(app.l(.hotkeyListener)) {
                    Text(app.hotkeyArmed ? app.l(.hotkeyBackendEventTap) : app.l(.hotkeyBackendNone))
                        .foregroundStyle(app.hotkeyArmed ? AnyShapeStyle(.primary) : AnyShapeStyle(.red))
                }
            }

            Section(app.l(.storageTitle)) {
                Toggle(app.l(.storageRetainAudio), isOn: $draft.retainAudio)
                Toggle(app.l(.storageAutoPaste), isOn: $draft.autoInsert)
                Toggle(app.l(.storageLiveInsert), isOn: $draft.liveInsert)
                // States the trade rather than advertising the feature. Live
                // insertion sends each phrase the moment it is decoded, so it
                // never gets the accuracy that comes from decoding the whole
                // recording at once — and text already in another application
                // cannot be taken back.
                Text(app.l(.storageLiveInsertHint))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .formStyle(.grouped)
    }

    private var localeBinding: Binding<UILocale> {
        Binding(get: { app.locale }, set: { app.setLocale($0) })
    }
}

// MARK: - Models

struct ModelSettings: View {
    @Bindable var app: AppModel
    @State private var download: ModelDownloadState?
    @State private var handle: (any CoreCancellable)?

    var body: some View {
        Form {
            Section(app.l(.modelsTitle)) {
                ForEach(app.models) { model in
                    row(model)
                }
            }

            Section(app.l(.runtimeTitle)) {
                LabeledContent(app.l(.runtimeEngine), value: "llama.cpp (Fun-ASR)")
                LabeledContent(app.l(.runtimeCompute), value: app.l(.runtimeComputeCPU))
                LabeledContent(
                    app.l(.runtimePlatform),
                    value: "\(app.core.platform.os) \(app.core.platform.arch)"
                )
                Label(
                    app.core.platform.bundledASR ? app.l(.runtimeReady) : app.l(.runtimeMissing),
                    systemImage: app.core.platform.bundledASR ? "cpu" : "exclamationmark.triangle"
                )
                .foregroundStyle(app.core.platform.bundledASR ? AnyShapeStyle(.primary) : AnyShapeStyle(.red))
            }
        }
        .formStyle(.grouped)
        .onDisappear { handle?.cancel() }
    }

    private func row(_ model: ModelInfo) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(model.name).fontWeight(.semibold)
                    Text(model.repoID)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Text(model.localPath)
                        .font(.caption.monospaced())
                        .foregroundStyle(.tertiary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
                Spacer()
                Text(Format.bytes(model.sizeBytes))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
                Button {
                    start(model)
                } label: {
                    Label(
                        model.status == .paused ? app.l(.resume) : app.l(model.status.message),
                        systemImage: "arrow.down.circle"
                    )
                }
                .disabled(model.status == .installed || download?.active == true)
            }

            if let error = model.lastError {
                Text(error).font(.caption).foregroundStyle(.red)
            }

            if let download, download.modelID == model.id {
                progress(download)
            }
        }
        .padding(.vertical, 2)
    }

    private func progress(_ state: ModelDownloadState) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            ProgressView(
                value: Double(state.downloadedBytes),
                total: Double(state.totalBytes ?? max(state.downloadedBytes, 1))
            )
            HStack {
                Text(state.message).font(.caption).foregroundStyle(.secondary)
                Spacer()
                Text(Format.downloadProgress(
                    downloaded: state.downloadedBytes,
                    total: state.totalBytes,
                    app.l
                ))
                .font(.caption)
                .foregroundStyle(.secondary)
                .monospacedDigit()
            }
        }
    }

    private func start(_ model: ModelInfo) {
        handle?.cancel()
        handle = app.core.models.download(modelID: model.id) { event in
            Task { @MainActor in receive(event) }
        }
    }

    @MainActor
    private func receive(_ event: ModelDownloadEvent) {
        switch event {
        case .started(let id, let downloaded, let total):
            download = ModelDownloadState(
                modelID: id, active: true, downloadedBytes: downloaded,
                totalBytes: total, message: app.l(.downloadModel)
            )
        case .progress(let id, let downloaded, let total):
            download = ModelDownloadState(
                modelID: id, active: true, downloadedBytes: downloaded,
                totalBytes: total, message: app.l(.downloadModel)
            )
        case .paused(let id, let downloaded, let total):
            download = ModelDownloadState(
                modelID: id, paused: true, downloadedBytes: downloaded,
                totalBytes: total, message: app.l(.downloadPaused)
            )
        case .finished(let id, _):
            download = ModelDownloadState(modelID: id, message: app.l(.downloadInstalled))
            Task { await app.refreshModels() }
        case .failed(let id, let text):
            // Verbatim: a download failure is the core's sentence, not a code
            // for this side to invent wording for.
            download = ModelDownloadState(modelID: id, message: text)
        }
    }
}

// MARK: - Services

/// Known-good endpoint/model pairs.
///
/// Typing a base URL from memory is the step where this feature gets abandoned.
/// Groq first: `whisper-large-v3-turbo` is the cheapest fast option. Ollama
/// needs no key at all, which makes the fully-offline cloud path one click away.
private let asrPresets: [(label: String, localised: Msg?, baseURL: String, model: String)] = [
    ("Groq", nil, "https://api.groq.com/openai/v1", "whisper-large-v3-turbo"),
    ("OpenAI", nil, "https://api.openai.com/v1", "gpt-4o-transcribe"),
    ("OpenRouter", nil, "https://openrouter.ai/api/v1", "whisper-large-v3-turbo"),
    ("Local (Ollama)", .cloudPresetLocal, "http://localhost:11434/v1", "whisper"),
]

struct ServiceSettings: View {
    @Bindable var app: AppModel
    @Bindable var draft: SettingsDraft

    var body: some View {
        Form {
            Section(app.l(.cloudTitle)) {
                Text(app.l(.cloudDesc))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)

                HStack(spacing: 6) {
                    ForEach(asrPresets, id: \.label) { preset in
                        Button(preset.localised.map { app.l($0) } ?? preset.label) {
                            draft.cloudBaseURL = preset.baseURL
                            draft.cloudModel = preset.model
                        }
                        .help("\(preset.baseURL) · \(preset.model)")
                    }
                }
                .controlSize(.small)

                TextField(app.l(.cloudBaseURL), text: $draft.cloudBaseURL, prompt: Text("https://api.groq.com/openai/v1"))
                TextField(app.l(.cloudModel), text: $draft.cloudModel, prompt: Text("whisper-large-v3-turbo"))
                TextField(
                    app.l(.cloudLanguageHint),
                    text: $draft.cloudLanguage,
                    prompt: Text(app.l(.cloudLanguageHintPlaceholder))
                )
                SecureField(
                    app.l(.cloudAPIKey),
                    text: $draft.cloudKeyDraft,
                    prompt: Text(app.l(.cloudAPIKeyPlaceholder))
                )
            }

            Section(app.l(.llmTitle)) {
                Text(app.l(.llmDesc))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)

                TextField(app.l(.llmBaseURL), text: $draft.llmBaseURL, prompt: Text("http://localhost:11434/v1"))
                TextField(app.l(.llmModel), text: $draft.llmModel, prompt: Text("qwen2.5:7b"))
                Picker(app.l(.llmPreset), selection: $draft.llmPreset) {
                    ForEach(app.llm.presets) { Text($0.label).tag($0.id) }
                }
                if let preset = app.llm.presets.first(where: { $0.id == draft.llmPreset }) {
                    Text(preset.description)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                HStack {
                    SecureField(
                        app.llm.hasAPIKey ? app.l(.llmAPIKeyStored) : app.l(.llmAPIKey),
                        text: $draft.llmKeyDraft,
                        prompt: Text(app.llm.hasAPIKey ? app.l(.llmAPIKeyKeep) : app.l(.llmAPIKeyOptional))
                    )
                    if app.llm.hasAPIKey {
                        Button(app.l(.clear)) {
                            Task {
                                try? await app.core.formatter.setAPIKey(nil)
                                app.llm = (try? await app.core.formatter.settings()) ?? app.llm
                            }
                        }
                    }
                }
                Text(app.l(.llmKeychain))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
    }
}

// MARK: - About

struct AboutSettings: View {
    @Bindable var app: AppModel

    var body: some View {
        Form {
            Section(app.l(.trialTitle)) {
                if let trial = app.trial {
                    if trial.licensed {
                        Text(app.l(.trialLicensed))
                    } else {
                        HStack {
                            Text(app.l(.trialUsed(minutes: Int(trial.usedSeconds / 60))))
                                .font(.title3.weight(.semibold))
                                .monospacedDigit()
                            Spacer()
                            Text(app.l(.trialLimit(minutes: Int((trial.limitSeconds / 60).rounded()))))
                                .foregroundStyle(.secondary)
                                .monospacedDigit()
                        }
                        ProgressView(value: trial.fraction)
                        Text(app.l(.trialNote))
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
            }

            // The one thing in this build that must not be mistaken for
            // working. It says so where somebody looking for the version number
            // will read it.
            Section(app.l(.appName)) {
                Text(app.l(.stubCoreNotice))
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                LabeledContent("Core", value: coreDescription)
            }
        }
        .formStyle(.grouped)
    }

    private var coreDescription: String {
        #if KOUSHU_HAS_RUST_CORE
        return "koushu-core (UniFFI) + Swift stubs"
        #else
        return "Swift stubs only"
        #endif
    }
}

extension Notification.Name {
    static let koushuSuspendHotkey = Notification.Name("koushu.suspendHotkey")
    static let koushuApplyHotkey = Notification.Name("koushu.applyHotkey")
}
