import Foundation

// In-memory stand-ins for the parts of the core that have not been extracted.
//
// Every type here is named `Stub…` on purpose. The one thing that must never
// happen is a screenshot or a report that treats one of these as evidence about
// the product: the search really searches, the archive really archives and the
// download really counts bytes, all of which is enough to build and judge the
// interface, and none of which says anything about whether transcription works.
//
// The transcripts these produce say so in their own text, so a picture of the
// app cannot be mistaken for a picture of it working.

// MARK: - Store

/// The shared mutable state, in one actor.
///
/// One actor rather than one per service because the real core is one database:
/// archiving a session has to be visible to the next search, and services that
/// each owned a slice of the data would let those drift in ways the real
/// implementation cannot.
public actor StubDatabase {
    private var sessions: [SessionInfo] = []
    private var transcripts: [String: [TranscriptInfo]] = [:]
    private var settings: [String: String] = [:]
    private var models: [ModelInfo] = []

    public init(seeded: Bool = true) {
        guard seeded else { return }
        // Built by a static function rather than a method: an actor's
        // initialiser is nonisolated, so it can assign to the properties but
        // cannot call anything that touches them.
        let seed = Self.makeSeed()
        sessions = seed.sessions
        transcripts = seed.transcripts
        models = seed.models
    }

    // MARK: Seed

    private static func makeSeed() -> (
        sessions: [SessionInfo],
        transcripts: [String: [TranscriptInfo]],
        models: [ModelInfo]
    ) {
        var sessions: [SessionInfo] = []
        var transcripts: [String: [TranscriptInfo]] = [:]
        let calendar = Calendar.current
        let now = Date()

        func day(_ offset: Int) -> Date {
            calendar.date(byAdding: .day, value: -offset, to: now) ?? now
        }

        let samples: [(String, Int, String, [String])] = [
            (
                "会议纪要", 0, "中文",
                [
                    "这是占位文本，不是真的转写结果——ASR 运行时还没有从 src-tauri 抽到共享核心里。",
                    "会话列表、搜索、筛选和归档都是真的在跑，只有语音这一段是假的。",
                ]
            ),
            (
                "Interface notes", 0, "English",
                [
                    "Placeholder text, not a real transcription. The ASR runtime has not been extracted into the shared core yet.",
                    "Everything around it is real: this row came out of a search index, and archiving it will hide it from the list.",
                ]
            ),
            (
                "周一想法", 1, "中文",
                [
                    "液态玻璃的意义全在它对背后内容的反应，单一背景根本看不出好坏。",
                ]
            ),
            (
                "Reading list", 3, "English",
                [
                    "The non-activating panel is the load-bearing part: if showing the bar steals focus, the text has nowhere to go.",
                ]
            ),
        ]

        let formatter = DateFormatter()
        formatter.dateFormat = "yyyy-MM-dd"
        formatter.locale = Locale(identifier: "en_US_POSIX")

        for (index, sample) in samples.enumerated() {
            let (title, dayOffset, language, lines) = sample
            let started = day(dayOffset)
            let id = "stub-session-\(index)"
            sessions.append(
                SessionInfo(
                    id: id,
                    title: title,
                    startedAt: started,
                    dateKey: formatter.string(from: started),
                    model: "fun-asr-nano-2512",
                    language: language,
                    runtime: ASRBackend.nano,
                    // One archived session so the archive scope has something to
                    // show. Without it the "Archived" filter is untestable and
                    // reads as broken.
                    archivedAt: index == 3 ? started : nil
                )
            )
            transcripts[id] = lines.enumerated().map { offset, text in
                TranscriptInfo(
                    id: "\(id)-t\(offset)",
                    sessionID: id,
                    text: text,
                    createdAt: started.addingTimeInterval(Double(offset) * 90),
                    durationMS: 4_000 + offset * 1_500,
                    model: "fun-asr-nano-2512",
                    language: language
                )
            }
        }

        let models = [
            ModelInfo(
                id: "fun-asr-nano-2512",
                name: "Fun-ASR-Nano 2512 (GGUF Q4_K)",
                backend: ASRBackend.nano,
                repoID: "FunAudioLLM/Fun-ASR-Nano-GGUF",
                localPath: NSHomeDirectory() + "/Library/Application Support/Fun ASR/models/fun-asr-nano-2512",
                status: .installed,
                sizeBytes: 940_000_000,
                installedAt: now
            ),
            ModelInfo(
                id: "sensevoice-small",
                name: "SenseVoiceSmall (GGUF Q8_0)",
                backend: ASRBackend.senseVoice,
                repoID: "FunAudioLLM/SenseVoiceSmall-GGUF",
                localPath: NSHomeDirectory() + "/Library/Application Support/Fun ASR/models/sensevoice-small",
                status: .available,
                sizeBytes: 480_000_000
            ),
        ]

        return (sessions, transcripts, models)
    }

    // MARK: Sessions

    func allSessions(limit: Int, filter: SessionFilter) -> [SessionInfo] {
        sessions
            .filter { matches($0, filter) }
            .sorted { $0.startedAt > $1.startedAt }
            .prefix(limit)
            .map { $0 }
    }

    private func matches(_ session: SessionInfo, _ filter: SessionFilter) -> Bool {
        switch filter.archived {
        case .active where session.isArchived: return false
        case .archived where !session.isArchived: return false
        default: break
        }
        if let language = filter.language, !language.isEmpty, session.language != language { return false }
        if let model = filter.model, !model.isEmpty, session.model != model { return false }
        if let from = filter.from, !from.isEmpty, session.dateKey < from { return false }
        if let to = filter.to, !to.isEmpty, session.dateKey > to { return false }
        return true
    }

    func insert(_ session: SessionInfo) {
        sessions.append(session)
        transcripts[session.id] = []
    }

    func transcripts(for sessionID: String) -> [TranscriptInfo] {
        transcripts[sessionID] ?? []
    }

    func append(_ transcript: TranscriptInfo) {
        transcripts[transcript.sessionID, default: []].append(transcript)
    }

    func setArchived(_ sessionID: String, _ archived: Bool) -> SessionInfo? {
        guard let index = sessions.firstIndex(where: { $0.id == sessionID }) else { return nil }
        sessions[index].archivedAt = archived ? Date() : nil
        return sessions[index]
    }

    func session(_ id: String) -> SessionInfo? {
        sessions.first { $0.id == id }
    }

    func options() -> FilterOptions {
        FilterOptions(
            languages: Array(Set(sessions.map(\.language))).sorted(),
            models: Array(Set(sessions.map(\.model))).sorted(),
            earliestDate: sessions.map(\.dateKey).min(),
            latestDate: sessions.map(\.dateKey).max(),
            archivedCount: sessions.count { $0.isArchived }
        )
    }

    func search(query: String, filter: SessionFilter, limit: Int) -> SearchResponse {
        let terms = query
            .split(whereSeparator: { $0.isWhitespace })
            .map(String.init)
            .filter { !$0.isEmpty }
        guard !terms.isEmpty else { return .empty }

        // The real store routes queries with a term shorter than three
        // characters away from the trigram index and scans instead. The stub
        // only reports which route it would have taken — the distinction is
        // there to explain an empty result, not to change how hits look.
        let mode: SearchMode = terms.allSatisfy { $0.count >= 3 } ? .fts : .substring

        var hits: [SearchHit] = []
        for session in sessions where matches(session, filter) {
            for transcript in transcripts[session.id] ?? [] {
                guard let range = firstMatch(of: terms, in: transcript.text) else { continue }
                hits.append(
                    SearchHit(
                        transcriptID: transcript.id,
                        sessionID: session.id,
                        sessionTitle: session.title,
                        dateKey: session.dateKey,
                        createdAt: transcript.createdAt,
                        language: transcript.language,
                        model: transcript.model,
                        archived: session.isArchived,
                        snippet: snippet(of: transcript.text, around: range)
                    )
                )
            }
        }
        hits.sort { $0.createdAt > $1.createdAt }
        return SearchResponse(
            terms: terms,
            hits: Array(hits.prefix(limit)),
            truncated: hits.count > limit,
            mode: mode
        )
    }

    func replaceFormatted(transcriptID: String, markdown: String, preset: String) -> TranscriptInfo? {
        for (sessionID, rows) in transcripts {
            guard let index = rows.firstIndex(where: { $0.id == transcriptID }) else { continue }
            transcripts[sessionID]?[index].formattedText = markdown
            transcripts[sessionID]?[index].formattedPreset = preset
            transcripts[sessionID]?[index].formattedAt = Date()
            return transcripts[sessionID]?[index]
        }
        return nil
    }

    // MARK: Settings

    func settingsSnapshot() -> [String: String] { settings }
    func setting(_ key: String) -> String? { settings[key] }
    func setSetting(_ key: String, _ value: String) { settings[key] = value }

    // MARK: Models

    func modelList() -> [ModelInfo] { models }

    func updateModel(_ model: ModelInfo) {
        guard let index = models.firstIndex(where: { $0.id == model.id }) else { return }
        models[index] = model
    }

    func model(_ id: String) -> ModelInfo? { models.first { $0.id == id } }
}

/// The window of text a hit is shown as, elided with `…`.
private func snippet(of text: String, around range: Range<String.Index>) -> String {
    let radius = 48
    let start = text.index(range.lowerBound, offsetBy: -radius, limitedBy: text.startIndex) ?? text.startIndex
    let end = text.index(range.upperBound, offsetBy: radius, limitedBy: text.endIndex) ?? text.endIndex
    var window = String(text[start..<end])
    if start > text.startIndex { window = "…" + window }
    if end < text.endIndex { window += "…" }
    return window
}

private func firstMatch(of terms: [String], in text: String) -> Range<String.Index>? {
    terms
        .compactMap { text.range(of: $0, options: [.caseInsensitive, .diacriticInsensitive]) }
        .min { $0.lowerBound < $1.lowerBound }
}

// MARK: - Services

public struct StubSessionStore: SessionStore {
    let database: StubDatabase
    public init(database: StubDatabase) { self.database = database }

    public func sessions(limit: Int, filter: SessionFilter) async throws -> [SessionInfo] {
        await database.allSessions(limit: limit, filter: filter)
    }

    public func createSession(title: String, model: String, language: String, runtime: String) async throws -> SessionInfo {
        let now = Date()
        let formatter = DateFormatter()
        formatter.dateFormat = "yyyy-MM-dd"
        formatter.locale = Locale(identifier: "en_US_POSIX")
        let session = SessionInfo(
            id: UUID().uuidString,
            title: title,
            startedAt: now,
            dateKey: formatter.string(from: now),
            model: model,
            language: language,
            runtime: runtime
        )
        await database.insert(session)
        return session
    }

    public func transcripts(sessionID: String) async throws -> [TranscriptInfo] {
        await database.transcripts(for: sessionID)
    }

    @discardableResult
    public func setArchived(sessionID: String, archived: Bool) async throws -> SessionInfo? {
        await database.setArchived(sessionID, archived)
    }

    public func filterOptions() async throws -> FilterOptions {
        await database.options()
    }

    public func search(query: String, filter: SessionFilter, limit: Int) async throws -> SearchResponse {
        await database.search(query: query, filter: filter, limit: limit)
    }

    @discardableResult
    public func appendTranscript(
        sessionID: String,
        text: String,
        model: String,
        language: String,
        durationMS: Int?
    ) async throws -> TranscriptInfo {
        let transcript = TranscriptInfo(
            id: UUID().uuidString,
            sessionID: sessionID,
            text: text,
            createdAt: Date(),
            durationMS: durationMS,
            model: model,
            language: language
        )
        await database.append(transcript)
        return transcript
    }

    @discardableResult
    public func saveFormatted(transcriptID: String, markdown: String, preset: String) async throws -> TranscriptInfo? {
        await database.replaceFormatted(transcriptID: transcriptID, markdown: markdown, preset: preset)
    }
}

public struct StubSettingsStore: SettingsStore {
    let database: StubDatabase
    public init(database: StubDatabase) { self.database = database }

    public func all() async throws -> [String: String] { await database.settingsSnapshot() }
    public func value(for key: String) async throws -> String? { await database.setting(key) }
    public func set(_ key: String, to value: String) async throws { await database.setSetting(key, value) }
}

public struct StubModelCatalog: ModelCatalog {
    let database: StubDatabase
    public init(database: StubDatabase) { self.database = database }

    public func models() async throws -> [ModelInfo] { await database.modelList() }

    /// Counts bytes on a timer so the progress UI has something real to render:
    /// a rate, a total, a pause and a resume. It downloads nothing.
    public func download(modelID: String, onEvent: @escaping @Sendable (ModelDownloadEvent) -> Void) -> CoreCancellable {
        let database = self.database
        let task = Task {
            guard var model = await database.model(modelID) else {
                onEvent(.failed(modelID: modelID, message: "No model with id \(modelID)."))
                return
            }
            let total = model.sizeBytes ?? 900_000_000
            onEvent(.started(modelID: modelID, downloadedBytes: 0, totalBytes: total))
            var downloaded: Int64 = 0
            let step = total / 40
            while downloaded < total {
                do {
                    try await Task.sleep(for: .milliseconds(120))
                } catch {
                    onEvent(.paused(modelID: modelID, downloadedBytes: downloaded, totalBytes: total))
                    return
                }
                downloaded = min(total, downloaded + step)
                onEvent(.progress(modelID: modelID, downloadedBytes: downloaded, totalBytes: total))
            }
            model.status = .installed
            model.installedAt = Date()
            await database.updateModel(model)
            onEvent(.finished(modelID: modelID, model: model))
        }
        return TaskCancellable(task: task)
    }

    public func pauseDownload(modelID: String) async {}
}

/// Returns placeholder text that says it is placeholder text.
///
/// Only used when the Rust core has not been linked in — `CORE=1 ./build.sh`
/// replaces it with `RustTranscriptionEngine`, which really decodes. It keeps
/// the shape of a real decode, including the pause between the key coming up
/// and the words appearing, because that pause is part of the interaction and a
/// UI built against an instant answer gets it wrong.
public struct StubTranscriptionEngine: TranscriptionEngine {
    public init() {}

    public func transcribe(
        _ request: TranscriptionRequest,
        onEvent: @escaping @Sendable (TranscriptionEvent) -> Void
    ) -> CoreCancellable {
        let sentence = Self.placeholder(for: request.language)
        let task = Task {
            // The whole recording is decoded in one pass, and that takes time
            // the interface has to be honest about.
            do { try await Task.sleep(for: .milliseconds(420)) } catch { return }
            onEvent(.committed(text: sentence, elapsedMS: 420))
        }
        return TaskCancellable(task: task)
    }

    /// Nothing is missing, because nothing is needed: this engine reads no
    /// model files. Saying otherwise would make the models screen report a
    /// problem that only exists in the build that cannot have it.
    public func missingAssets(modelID: String, backend: String) -> [String] { [] }

    public static func placeholder(for language: String) -> String {
        language == "\u{4e2d}\u{6587}" || language == "\u{7ca4}\u{8bed}"
            ? "\u{8fd9}\u{662f}\u{5360}\u{4f4d}\u{6587}\u{672c}\u{ff0c}\u{4e0d}\u{662f}\u{771f}\u{7684}\u{8f6c}\u{5199}\u{7ed3}\u{679c}\u{2014}\u{2014}\u{6ca1}\u{6709}\u{94fe}\u{63a5} Rust \u{6838}\u{5fc3}\u{ff0c}\u{7528} CORE=1 ./build.sh \u{91cd}\u{5efa}\u{3002}"
            : "Placeholder text, not a real transcription \u{2014} this build was made without the Rust core. Rebuild with CORE=1 ./build.sh."
    }
}

public struct StubLLMFormatter: LLMFormatter {
    let database: StubDatabase
    public init(database: StubDatabase) { self.database = database }

    public func settings() async throws -> LLMSettings {
        let stored = await database.settingsSnapshot()
        return LLMSettings(
            baseURL: stored[SettingKey.llmBaseURL] ?? "",
            model: stored[SettingKey.llmModel] ?? "",
            hasAPIKey: false,
            preset: stored[SettingKey.llmPreset] ?? "typeset",
            autoFormat: false,
            presets: FormatPreset.builtIn
        )
    }

    public func setAPIKey(_ key: String?) async throws {}

    public func format(
        transcriptID: String?,
        text: String,
        preset: String?,
        onEvent: @escaping @Sendable (FormatEvent) -> Void
    ) -> CoreCancellable {
        let database = self.database
        let task = Task {
            // Not an LLM: it wraps the text and streams it back a piece at a
            // time. That exercises the only thing the UI is responsible for —
            // rendering a stream that can fail halfway — without pretending
            // anything was typeset.
            let markdown = "> \(text)\n\n_(占位排版 / placeholder formatting — the LLM client has not been wired to the core yet.)_"
            var sent = ""
            for piece in markdown.split(separator: " ", omittingEmptySubsequences: false).map({ String($0) + " " }) {
                do { try await Task.sleep(for: .milliseconds(35)) } catch { return }
                sent += piece
                onEvent(.delta(piece))
            }
            if let transcriptID {
                _ = await database.replaceFormatted(
                    transcriptID: transcriptID,
                    markdown: sent,
                    preset: preset ?? "typeset"
                )
            }
            onEvent(.done(sent))
        }
        return TaskCancellable(task: task)
    }
}

/// Rejects everything, with the same wording the Rust core uses for a build
/// with no key configured. The real implementation is `RustLicenseService`,
/// compiled in when the bindings have been generated.
public struct StubLicenseService: LicenseService {
    public init() {}
    public func verify(_ license: String) -> LicenseInfo {
        LicenseInfo(valid: false, detail: "This build has no licence key configured.")
    }
}

public struct StubTrialMeter: TrialMeter {
    public init() {}
    public func status() async throws -> TrialStatus {
        TrialStatus(usedSeconds: 372, limitSeconds: 1800, licensed: false, firstTranscript: false)
    }
}

final class TaskCancellable: CoreCancellable, @unchecked Sendable {
    private let task: Task<Void, Never>
    init(task: Task<Void, Never>) { self.task = task }
    func cancel() { task.cancel() }
}

// MARK: - Presets

extension FormatPreset {
    /// Mirrors `core/src/llm/presets.rs`.
    ///
    /// Duplicated rather than read across the boundary because the preset list
    /// is not yet exported through UniFFI. When it is, this disappears; until
    /// then the ids are the part that has to match, since they are what gets
    /// written to `llm.preset`.
    public static let builtIn: [FormatPreset] = [
        FormatPreset(
            id: "typeset",
            label: "Typeset",
            description: "Paragraphs, punctuation and lists. Wording is left alone."
        ),
        FormatPreset(
            id: "notes",
            label: "Notes",
            description: "Condensed into bullet points, keeping every fact."
        ),
        FormatPreset(
            id: "email",
            label: "Email",
            description: "Rewritten as a short message, in the same voice."
        ),
    ]
}
