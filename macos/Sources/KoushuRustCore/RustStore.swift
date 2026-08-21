import Foundation
import KoushuCore

/// Sessions, transcripts, search, settings and models — against the real
/// SQLite database, shared with the Tauri build.
///
/// Three protocols, one `Store`, because they are one file: archiving a session
/// has to be visible to the next search, and a settings write has to be visible
/// to the next read. Splitting the connection would let those drift in ways the
/// database itself cannot.
///
/// Everything here is an adapter and nothing here is logic. The interesting
/// parts — the trigram routing rule, the title derivation, the AND semantics
/// across both search paths — are in `koushu-core::storage` where there can
/// only be one copy of them. What this file does is translate: RFC 3339 strings
/// into `Date`, `Optional` into "absent", and the generated record types into
/// the domain types the views are written against.
///
/// The translation is not ceremony. The generated types are regenerated on
/// every build from whatever Rust currently says; the domain types are what a
/// hundred call sites reference. Without this layer, adding a column in Rust
/// would ripple into view code.
public struct RustStore: SessionStore, SettingsStore, ModelCatalog {
    let store: Store

    /// - Parameter databasePath: the SQLite file. Created, and migrated, if it
    ///   does not exist yet.
    public init(databasePath: String) throws {
        store = try Store.open(path: databasePath)
    }

    /// Drop sessions that never captured anything.
    ///
    /// Holding the key and changing your mind leaves an empty session behind,
    /// and left alone the sidebar fills with rows containing nothing. Run at
    /// startup rather than on close, because a crash is exactly when one gets
    /// left over.
    @discardableResult
    public func pruneEmptySessions() -> Int {
        Int((try? store.pruneEmptySessions()) ?? 0)
    }

    // MARK: - SessionStore

    public func sessions(limit: Int, filter: KoushuCore.SessionFilter) async throws -> [SessionInfo] {
        try store.sessions(limit: Int64(limit), filter: filter.wire).map(\.domain)
    }

    public func createSession(
        title: String,
        model: String,
        language: String,
        runtime: String
    ) async throws -> SessionInfo {
        try store.createSession(title: title, model: model, language: language, runtime: runtime).domain
    }

    public func transcripts(sessionID: String) async throws -> [TranscriptInfo] {
        try store.transcripts(sessionId: sessionID).map(\.domain)
    }

    @discardableResult
    public func setArchived(sessionID: String, archived: Bool) async throws -> SessionInfo? {
        try store.setArchived(sessionId: sessionID, archived: archived)?.domain
    }

    public func filterOptions() async throws -> KoushuCore.FilterOptions {
        let options = try store.filterOptions()
        return KoushuCore.FilterOptions(
            languages: options.languages,
            models: options.models,
            earliestDate: options.earliestDate,
            latestDate: options.latestDate,
            archivedCount: Int(options.archivedCount)
        )
    }

    public func search(
        query: String,
        filter: KoushuCore.SessionFilter,
        limit: Int
    ) async throws -> SearchResponse {
        let results = try store.search(query: query, filter: filter.wire, limit: Int64(limit))
        return SearchResponse(
            terms: results.terms,
            hits: results.hits.map(\.domain),
            truncated: results.truncated,
            mode: results.mode.domain
        )
    }

    @discardableResult
    public func appendTranscript(
        sessionID: String,
        text: String,
        model: String,
        language: String,
        durationMS: Int?
    ) async throws -> TranscriptInfo {
        try store.appendTranscript(
            sessionId: sessionID,
            text: text,
            model: model,
            language: language,
            durationMs: durationMS.map(Int64.init)
        ).domain
    }

    @discardableResult
    public func saveFormatted(
        transcriptID: String,
        markdown: String,
        preset: String
    ) async throws -> TranscriptInfo? {
        try store.saveFormatted(transcriptId: transcriptID, markdown: markdown, preset: preset)?.domain
    }

    // MARK: - SettingsStore

    public func all() async throws -> [String: String] {
        Dictionary(try store.settings().map { ($0.key, $0.value) }, uniquingKeysWith: { _, last in last })
    }

    public func value(for key: String) async throws -> String? {
        try store.setting(key: key)
    }

    public func set(_ key: String, to value: String) async throws {
        try store.setSetting(key: key, value: value)
    }

    // MARK: - ModelCatalog

    public func models() async throws -> [ModelInfo] {
        try store.models().map(\.domain)
    }

    /// Put the built-in catalogue in place if it is not there.
    ///
    /// `INSERT OR IGNORE` on the Rust side, so a row the user already has — with
    /// its download status and real installed size — is never overwritten.
    public func seedModels(_ models: [ModelInfo]) throws {
        try store.seedModels(models: models.map(\.wire))
    }

    /// Downloading is not implemented here yet.
    ///
    /// It is the one thing in this file that would need network code rather than
    /// translation, and it belongs in the core beside the model catalogue it
    /// updates. Reporting a failure the user can act on beats silently doing
    /// nothing and leaving a progress bar at zero forever.
    public func download(
        modelID: String,
        onEvent: @escaping @Sendable (ModelDownloadEvent) -> Void
    ) -> CoreCancellable {
        onEvent(.failed(
            modelID: modelID,
            message: "Downloading is not wired up in the native build yet. Use the Tauri app to fetch a model; both share this models directory."
        ))
        return NoopCancellable()
    }

    public func pauseDownload(modelID: String) async {}
}

final class NoopCancellable: CoreCancellable {
    func cancel() {}
}

// MARK: - Translation

/// RFC 3339 is what is in the column, so it is what crosses the boundary.
///
/// Two formatters because the strings on disk come from two writers: Rust's
/// `chrono` emits fractional seconds, the older rows do not, and `ISO8601`
/// parsing is exact — a formatter configured for one silently returns nil for
/// the other, which would show up as every timestamp reading as "now".
/// `ISO8601DateFormatter` is a class and is not `Sendable`, so it cannot be a
/// global `let` under Swift 6. Wrapped in a lock rather than made thread-local:
/// these are called from the main actor in practice, the work is microseconds,
/// and a lock is the version that stays correct if that ever stops being true.
private enum Timestamps {
    nonisolated(unsafe) private static let withFractional: ISO8601DateFormatter = {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter
    }()

    nonisolated(unsafe) private static let plain: ISO8601DateFormatter = {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime]
        return formatter
    }()

    private static let lock = NSLock()

    static func parse(_ value: String) -> Date? {
        lock.withLock { withFractional.date(from: value) ?? plain.date(from: value) }
    }

    static func string(from date: Date) -> String {
        lock.withLock { withFractional.string(from: date) }
    }
}

private func parseTimestamp(_ value: String) -> Date? {
    Timestamps.parse(value)
}

extension SessionRecord {
    var domain: SessionInfo {
        SessionInfo(
            id: id,
            title: title,
            // A row whose timestamp will not parse is a real row with a broken
            // field; falling back to `.distantPast` keeps it visible and sorts
            // it to the bottom rather than hiding somebody's transcript.
            startedAt: parseTimestamp(startedAt) ?? .distantPast,
            endedAt: endedAt.flatMap(parseTimestamp),
            dateKey: dateKey,
            model: model,
            language: language,
            runtime: runtime,
            archivedAt: archivedAt.flatMap(parseTimestamp)
        )
    }
}

extension TranscriptRecord {
    var domain: TranscriptInfo {
        TranscriptInfo(
            id: id,
            sessionID: sessionId,
            text: text,
            status: status,
            source: source,
            createdAt: parseTimestamp(createdAt) ?? .distantPast,
            durationMS: durationMs.map(Int.init),
            model: model,
            language: language,
            formattedText: formattedText,
            formattedPreset: formattedPreset,
            formattedAt: formattedAt.flatMap(parseTimestamp)
        )
    }
}

extension KoushuRustCore.SearchHit {
    var domain: KoushuCore.SearchHit {
        KoushuCore.SearchHit(
            transcriptID: transcriptId,
            sessionID: sessionId,
            sessionTitle: sessionTitle,
            dateKey: dateKey,
            createdAt: parseTimestamp(createdAt) ?? .distantPast,
            language: language,
            model: model,
            archived: archived,
            snippet: snippet
        )
    }
}

extension KoushuRustCore.SearchMode {
    var domain: KoushuCore.SearchMode {
        switch self {
        case .empty: .empty
        case .fts: .fts
        case .substring: .substring
        }
    }
}

extension ModelRecord {
    var domain: ModelInfo {
        ModelInfo(
            id: id,
            name: name,
            backend: backend,
            source: source,
            repoID: repoId,
            localPath: localPath,
            // An unrecognised status is shown as `.error` rather than dropped:
            // the row exists, and a model in a state this build does not know
            // about is exactly the thing worth surfacing.
            status: ModelStatus(rawValue: status) ?? .error,
            sizeBytes: sizeBytes,
            installedAt: installedAt.flatMap(parseTimestamp),
            lastError: lastError
        )
    }
}

extension ModelInfo {
    var wire: ModelRecord {
        ModelRecord(
            id: id,
            name: name,
            backend: backend,
            source: source,
            repoId: repoID,
            localPath: localPath,
            status: status.rawValue,
            sizeBytes: sizeBytes,
            installedAt: installedAt.map(Timestamps.string(from:)),
            lastError: lastError
        )
    }
}

extension KoushuCore.SessionFilter {
    var wire: KoushuRustCore.SessionFilter {
        KoushuRustCore.SessionFilter(
            language: language,
            model: model,
            from: from,
            to: to,
            archived: wireScope
        )
    }

    private var wireScope: KoushuRustCore.ArchiveScope {
        switch archived {
        case .active: .active
        case .archived: .archived
        case .all: .all
        }
    }
}
