import KoushuCore
import SwiftUI

/// The settings window.
///
/// Four categories, because that is how many groups the panels genuinely fall
/// into — the count is not a target. Everything that decides what the next
/// recording does is General; the things you install are Models; the two
/// OpenAI-compatible endpoints are both Services and neither is used by most
/// people; the rest is status you read rather than settings you change.
///
/// A plain `TabView` rather than a hand-built rail: on macOS this is the settings
/// window shape the platform already draws, and matching it costs nothing.
struct SettingsWindowView: View {
    @Bindable var app: AppModel
    @State private var draft = SettingsDraft()
    @State private var message: String = ""

    var body: some View {
        VStack(spacing: 0) {
            TabView {
                Tab(app.l(.tabGeneral), systemImage: "slider.horizontal.3") {
                    GeneralSettings(app: app, draft: draft)
                }
                Tab(app.l(.tabModels), systemImage: "shippingbox") {
                    ModelSettings(app: app)
                }
                Tab(app.l(.tabServices), systemImage: "cloud") {
                    ServiceSettings(app: app, draft: draft)
                }
                Tab(app.l(.tabAbout), systemImage: "info.circle") {
                    AboutSettings(app: app)
                }
            }

            // Save lives here rather than inside one panel. Once the settings
            // are split across four tabs, a button parked in Storage would
            // silently be the thing that commits the Services form two tabs
            // away — and the language picker and the hotkey recorder are the
            // exceptions precisely because they cannot wait for it.
            Divider()
            HStack {
                Text(message)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                Spacer()
                Button(app.l(.storageSave)) {
                    Task { message = await draft.commit(to: app) }
                }
                .keyboardShortcut("s")
                .buttonStyle(.borderedProminent)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
        }
        .frame(minWidth: 660, minHeight: 440)
        .task { await draft.load(from: app) }
        .environment(\.locale, Locale(identifier: app.locale == .zh ? "zh-Hans" : "en"))
    }
}

/// The form's own state, separate from what is stored.
///
/// A settings form has to be editable without each keystroke being a write —
/// half-typed URLs and API keys are not settings — so the drafts live here and
/// `commit` is the only thing that writes.
@MainActor
@Observable
final class SettingsDraft {
    var retainAudio = false
    var autoInsert = true
    var liveInsert = false

    var cloudBaseURL = ""
    var cloudModel = ""
    var cloudLanguage = ""
    /// Never populated from storage. The stored key is write-only from here:
    /// showing it back would mean it had crossed the boundary at all.
    var cloudKeyDraft = ""

    var llmBaseURL = ""
    var llmModel = ""
    var llmPreset = "typeset"
    var llmKeyDraft = ""

    func load(from app: AppModel) async {
        let stored = (try? await app.core.settings.all()) ?? [:]
        retainAudio = app.retainAudio
        autoInsert = app.autoInsert
        liveInsert = app.liveInsert
        cloudBaseURL = stored[SettingKey.cloudBaseURL] ?? ""
        cloudModel = stored[SettingKey.cloudModel] ?? ""
        cloudLanguage = stored[SettingKey.cloudLanguage] ?? ""
        llmBaseURL = app.llm.baseURL
        llmModel = app.llm.model
        llmPreset = app.llm.preset
    }

    /// Returns the sentence to show in the footer.
    func commit(to app: AppModel) async -> String {
        app.retainAudio = retainAudio
        app.autoInsert = autoInsert
        app.liveInsert = liveInsert

        await app.save(SettingKey.retainAudio, retainAudio ? "true" : "false")
        await app.save(SettingKey.autoPaste, autoInsert ? "true" : "false")
        await app.save(SettingKey.liveInsert, liveInsert ? "true" : "false")

        await app.save(SettingKey.cloudBaseURL, cloudBaseURL.trimmed)
        await app.save(SettingKey.cloudModel, cloudModel.trimmed)
        await app.save(SettingKey.cloudLanguage, cloudLanguage.trimmed)

        await app.save(SettingKey.llmBaseURL, llmBaseURL.trimmed)
        await app.save(SettingKey.llmModel, llmModel.trimmed)
        await app.save(SettingKey.llmPreset, llmPreset)

        // An empty draft means "leave the stored key alone"; clearing is a
        // separate, explicit action.
        if !llmKeyDraft.trimmed.isEmpty {
            try? await app.core.formatter.setAPIKey(llmKeyDraft.trimmed)
            llmKeyDraft = ""
        }

        app.llm = (try? await app.core.formatter.settings()) ?? app.llm
        return app.l(.settingsSaved)
    }
}

extension String {
    var trimmed: String { trimmingCharacters(in: .whitespacesAndNewlines) }
}
