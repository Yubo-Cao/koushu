import Foundation
import KoushuCore
import Observation

/// The session list, the search over it, and the filters that narrow both.
///
/// One object rather than three pieces of view state, so the query, the filters
/// and the results cannot drift out of step: the same filter narrows the list
/// and the search, and a stale combination of the two would show hits belonging
/// to sessions the sidebar is hiding — hits the user then cannot open.
@MainActor
@Observable
final class SessionBrowser {
    private let core: CoreServices

    var sessions: [SessionInfo] = []
    var activeSessionID: String?
    var transcripts: [TranscriptInfo] = []

    var query: String = "" {
        didSet { if query != oldValue { runSearch() } }
    }
    var filter: SessionFilter = .none {
        didSet { if filter != oldValue { reload(); runSearch() } }
    }
    var results: SearchResponse?
    var options: FilterOptions = .empty

    /// Streaming Markdown per transcript while a formatting pass is in flight.
    var formatting: [String: String] = [:]
    var formatErrors: [String: String] = [:]
    private var formatHandles: [String: any CoreCancellable] = [:]

    /// A search hit whose session has been selected but whose transcripts have
    /// not arrived yet. The scroll cannot happen when the hit is clicked — the
    /// row does not exist — so it is remembered until the list it lives in does.
    var pendingScroll: String?

    /// Every keystroke fires a query; only the newest answer may be rendered.
    /// A local index replies out of order rarely, but "rarely" here means the
    /// results flicker back to a previous prefix, which looks like a bug.
    private var generation = 0
    private var searchTask: Task<Void, Never>?

    init(core: CoreServices) {
        self.core = core
    }

    var isSearching: Bool {
        !query.trimmingCharacters(in: .whitespaces).isEmpty
    }

    var activeSession: SessionInfo? {
        sessions.first { $0.id == activeSessionID }
    }

    /// Sessions grouped by the day they belong to, newest day first.
    var grouped: [(dateKey: String, sessions: [SessionInfo])] {
        var order: [String] = []
        var buckets: [String: [SessionInfo]] = [:]
        for session in sessions {
            if buckets[session.dateKey] == nil { order.append(session.dateKey) }
            buckets[session.dateKey, default: []].append(session)
        }
        return order.map { ($0, buckets[$0] ?? []) }
    }

    // MARK: Loading

    func start() async {
        await reloadAsync()
        options = (try? await core.sessions.filterOptions()) ?? .empty
    }

    func reload() {
        Task { await reloadAsync() }
    }

    /// Re-list, keeping the active session if it survives the new filter.
    ///
    /// Tightening a filter must never yank the transcript pane out from under
    /// whatever is being read; only a filter that actually excludes the open
    /// session moves the selection.
    func reloadAsync() async {
        let next = (try? await core.sessions.sessions(limit: 200, filter: filter)) ?? []
        sessions = next
        if let current = activeSessionID, next.contains(where: { $0.id == current }) {
            return
        }
        await select(next.first?.id)
    }

    func select(_ sessionID: String?) async {
        activeSessionID = sessionID
        guard let sessionID else {
            transcripts = []
            return
        }
        transcripts = (try? await core.sessions.transcripts(sessionID: sessionID)) ?? []
    }

    func refreshTranscripts() async {
        guard let id = activeSessionID else { return }
        transcripts = (try? await core.sessions.transcripts(sessionID: id)) ?? []
    }

    func refreshOptions() async {
        options = (try? await core.sessions.filterOptions()) ?? .empty
    }

    // MARK: Search

    private func runSearch() {
        searchTask?.cancel()
        let term = query.trimmingCharacters(in: .whitespaces)
        guard !term.isEmpty else {
            // Not a "no matches" state — there is nothing to match yet.
            // Clearing the results puts the plain session list back.
            generation += 1
            results = nil
            return
        }
        generation += 1
        let mine = generation
        searchTask = Task {
            // No loading state on purpose: a local index answers in single-digit
            // milliseconds, and a spinner that appears and vanishes on every
            // keystroke is noise, not feedback.
            let response = try? await core.sessions.search(query: term, filter: filter, limit: 80)
            guard !Task.isCancelled, generation == mine else { return }
            results = response
        }
    }

    func clearSearch() {
        query = ""
    }

    func resetFilters() {
        filter = .none
    }

    /// Open the session a hit belongs to, then scroll to the transcript.
    func open(hit: SearchHit) async {
        if sessions.contains(where: { $0.id == hit.sessionID }) {
            await select(hit.sessionID)
        } else {
            // The hit is in a session the sidebar filter is hiding — an archived
            // one, most often. Fetch it so it can still be opened.
            let all = (try? await core.sessions.sessions(
                limit: 200,
                filter: SessionFilter(
                    language: filter.language,
                    model: filter.model,
                    from: filter.from,
                    to: filter.to,
                    archived: .all
                )
            )) ?? []
            if let found = all.first(where: { $0.id == hit.sessionID }) {
                sessions.append(found)
                await select(found.id)
            }
        }
        clearSearch()
        pendingScroll = hit.transcriptID
    }

    // MARK: Sessions

    func createSession(title: String, model: String, language: String, runtime: String) async -> SessionInfo? {
        guard let session = try? await core.sessions.createSession(
            title: title,
            model: model,
            language: language,
            runtime: runtime
        ) else { return nil }
        await reloadAsync()
        await select(session.id)
        await refreshOptions()
        return session
    }

    /// The session a new utterance belongs to, creating one if there is none.
    func ensureSession(untitled: String, model: String, language: String, runtime: String) async -> SessionInfo? {
        if let active = activeSession { return active }
        return await createSession(title: untitled, model: model, language: language, runtime: runtime)
    }

    func setArchived(_ session: SessionInfo, _ archived: Bool) async {
        _ = try? await core.sessions.setArchived(sessionID: session.id, archived: archived)
        await reloadAsync()
        await refreshOptions()
    }

    /// Store a finished transcript and show it if its session is open.
    ///
    /// The row shown is the one the store returned, not one built here: the id
    /// and the timestamp are the store's to assign, and displaying a locally
    /// invented row would put something on screen the database does not contain
    /// — which then fails to match when the session is next opened.
    @discardableResult
    func appendTranscript(
        sessionID: String,
        text: String,
        model: String,
        language: String,
        durationMS: Int?
    ) async -> TranscriptInfo? {
        guard let stored = try? await core.sessions.appendTranscript(
            sessionID: sessionID,
            text: text,
            model: model,
            language: language,
            durationMS: durationMS
        ) else { return nil }

        if stored.sessionID == activeSessionID {
            transcripts.append(stored)
        }
        // A session's title is derived from its first transcript, and the store
        // is what derives it, so the sidebar has to be re-read rather than
        // patched.
        await reloadAsync()
        return stored
    }

    // MARK: Formatting

    func format(_ transcript: TranscriptInfo, preset: String?) {
        let id = transcript.id
        formatErrors[id] = nil
        formatting[id] = ""

        let handle = core.formatter.format(
            transcriptID: id,
            text: transcript.text,
            preset: preset
        ) { [weak self] event in
            Task { @MainActor in
                guard let self else { return }
                switch event {
                case .delta(let piece):
                    self.formatting[id, default: ""] += piece
                case .done:
                    self.formatting[id] = nil
                    self.formatHandles[id] = nil
                    // Re-read so the stored text becomes the source of truth
                    // rather than whatever the stream happened to accumulate.
                    await self.refreshTranscripts()
                case .failed(let message):
                    self.formatErrors[id] = message
                    self.formatting[id] = nil
                    self.formatHandles[id] = nil
                }
            }
        }
        formatHandles[id] = handle
    }

    func cancelFormatting() {
        for handle in formatHandles.values { handle.cancel() }
        formatHandles.removeAll()
        formatting.removeAll()
    }
}
